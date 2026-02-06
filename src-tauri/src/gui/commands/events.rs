//! Worker event streaming commands
//!
//! Commands for real-time worker event streaming via ACP.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;
use tracing::info;

use crate::core::api_types::{WorkerEventResponse, WorkerEventsResponse};
use crate::core::{config, state::SQLiteState};

use super::get_run_state;

/// Manages active worker event streams
pub struct WorkerEventStreamManager {
    /// Active streams: (run_name, worker_name) -> cancel sender
    streams: Mutex<HashMap<(String, String), oneshot::Sender<()>>>,
}

impl WorkerEventStreamManager {
    pub fn new() -> Self {
        Self {
            streams: Mutex::new(HashMap::new()),
        }
    }

    /// Check if a stream is active for this worker
    pub fn is_active(&self, run_name: &str, worker_name: &str) -> bool {
        let streams = self.streams.lock().unwrap();
        streams.contains_key(&(run_name.to_string(), worker_name.to_string()))
    }

    /// Register a new stream
    pub fn register(&self, run_name: &str, worker_name: &str, cancel_tx: oneshot::Sender<()>) {
        let mut streams = self.streams.lock().unwrap();
        streams.insert((run_name.to_string(), worker_name.to_string()), cancel_tx);
    }

    /// Stop and remove a stream
    pub fn stop(&self, run_name: &str, worker_name: &str) -> bool {
        let mut streams = self.streams.lock().unwrap();
        if let Some(cancel_tx) = streams.remove(&(run_name.to_string(), worker_name.to_string())) {
            // Send cancel signal (ignore error if receiver dropped)
            let _ = cancel_tx.send(());
            true
        } else {
            false
        }
    }

    /// Remove a stream without sending cancel (for cleanup after stream ends)
    pub fn remove(&self, run_name: &str, worker_name: &str) {
        let mut streams = self.streams.lock().unwrap();
        streams.remove(&(run_name.to_string(), worker_name.to_string()));
    }
}

impl Default for WorkerEventStreamManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Stream status event emitted to frontend
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum WorkerStreamEvent {
    /// Initial batch of historical events
    #[serde(rename = "history")]
    History {
        #[serde(rename = "runName")]
        run_name: String,
        #[serde(rename = "workerName")]
        worker_name: String,
        events: Vec<WorkerEventResponse>,
        #[serde(rename = "workerStatus")]
        worker_status: Option<String>,
    },
    /// New event during live streaming
    #[serde(rename = "event")]
    Event {
        #[serde(rename = "runName")]
        run_name: String,
        #[serde(rename = "workerName")]
        worker_name: String,
        event: WorkerEventResponse,
    },
    /// Worker status update
    #[serde(rename = "status")]
    Status {
        #[serde(rename = "runName")]
        run_name: String,
        #[serde(rename = "workerName")]
        worker_name: String,
        #[serde(rename = "workerStatus")]
        worker_status: Option<String>,
    },
    /// Stream ended
    #[serde(rename = "ended")]
    Ended {
        #[serde(rename = "runName")]
        run_name: String,
        #[serde(rename = "workerName")]
        worker_name: String,
    },
}

/// Get worker events for real-time streaming
///
/// Returns events since `after_id` for efficient polling.
/// On first call, pass `after_id: null` to get recent events.
#[tracing::instrument]
#[tauri::command]
pub async fn get_worker_events(
    run_name: String,
    worker_name: String,
    after_id: Option<i64>,
    limit: Option<i64>,
) -> Result<WorkerEventsResponse, String> {
    // Return empty response if run doesn't exist (graceful handling)
    let state = match get_run_state(&run_name).await {
        Ok(s) => s,
        Err(_) => {
            return Ok(WorkerEventsResponse {
                events: Vec::new(),
                last_id: None,
                worker_status: None,
            });
        }
    };

    let limit = limit.unwrap_or(1000);
    let events = state
        .get_worker_events(&worker_name, after_id, limit)
        .await
        .map_err(|e| format!("Failed to get worker events: {}", e))?;

    let last_id = events.last().map(|e| e.id);

    // Get worker status to determine if still streaming
    let worker_status = state
        .get_worker(&worker_name)
        .await
        .ok()
        .flatten()
        .map(|w| w.status.as_str().to_string());

    let events: Vec<WorkerEventResponse> = events
        .into_iter()
        .map(|e| WorkerEventResponse {
            id: e.id,
            worker_name: e.worker_name,
            event_type: e.event_type.as_str().to_string(),
            timestamp: e.timestamp,
            content: e.content,
            tool_call_id: e.tool_call_id,
            tool_title: e.tool_title,
            tool_kind: e.tool_kind,
            tool_status: e
                .tool_status
                .map(|s: crate::core::state::ToolCallStatus| s.as_str().to_string()),
            tool_input: e.tool_input,
            tool_output: e.tool_output,
        })
        .collect();

    Ok(WorkerEventsResponse {
        events,
        last_id,
        worker_status,
    })
}

/// Clear worker events (for cleanup when attaching/detaching)
#[tracing::instrument]
#[tauri::command]
pub async fn clear_worker_events(run_name: String, worker_name: String) -> Result<(), String> {
    // Return Ok if run doesn't exist (graceful handling)
    let state = match get_run_state(&run_name).await {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    state
        .clear_worker_events(&worker_name)
        .await
        .map_err(|e| format!("Failed to clear worker events: {}", e))?;

    Ok(())
}

/// Start streaming worker events to the frontend
///
/// This fetches historical events first, then polls for new ones.
/// Events are emitted as `worker-event` Tauri events.
#[tracing::instrument(skip(app, stream_manager))]
#[tauri::command]
pub async fn start_worker_event_stream(
    app: tauri::AppHandle,
    stream_manager: tauri::State<'_, std::sync::Arc<WorkerEventStreamManager>>,
    run_name: String,
    worker_name: String,
) -> Result<(), String> {
    use tauri::Emitter;

    info!(
        "[WorkerStream] start_worker_event_stream called for {}/{}",
        run_name, worker_name
    );

    // Stop any existing stream for this worker
    stream_manager.stop(&run_name, &worker_name);

    let db_path = config::run_dir(&run_name).join("hirsel.db");
    info!(
        "[WorkerStream] DB path: {:?}, exists: {}",
        db_path,
        db_path.exists()
    );
    if !db_path.exists() {
        return Err(format!("Run database not found: {}", run_name));
    }

    // Create cancel channel
    let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
    stream_manager.register(&run_name, &worker_name, cancel_tx);

    let run_name_clone = run_name.clone();
    let worker_name_clone = worker_name.clone();
    let stream_manager_clone = stream_manager.inner().clone();
    let app_clone = app.clone();

    // Spawn background task to stream events
    info!("[WorkerStream] Spawning background task");
    tokio::spawn(async move {
        let app = app_clone;
        info!(
            "[WorkerStream] Background task started for {}/{}",
            run_name_clone, worker_name_clone
        );
        let mut last_id: Option<i64> = None;
        let mut last_status: Option<String> = None;
        let poll_interval = tokio::time::Duration::from_millis(200);
        let mut first_poll = true;

        loop {
            // Check for cancellation
            if cancel_rx.try_recv().is_ok() {
                info!(
                    "[WorkerStream] Cancelled for {}/{}",
                    run_name_clone, worker_name_clone
                );
                break;
            }

            // Poll for events
            let state = match SQLiteState::new(&run_name_clone).await {
                Ok(s) => s,
                Err(e) => {
                    info!("[WorkerStream] Failed to open database: {}", e);
                    break;
                }
            };

            let events = match state
                .get_worker_events(&worker_name_clone, last_id, 1000)
                .await
            {
                Ok(e) => e,
                Err(e) => {
                    info!("[WorkerStream] Failed to get events: {}", e);
                    break;
                }
            };

            if first_poll {
                info!(
                    "[WorkerStream] First poll: got {} events, worker_status query next",
                    events.len()
                );
            }

            // Get worker status
            let worker_status = state
                .get_worker(&worker_name_clone)
                .await
                .ok()
                .flatten()
                .map(|w| w.status.as_str().to_string());

            if !events.is_empty() {
                let old_last_id = last_id;
                last_id = events.last().map(|e| e.id);

                info!(
                    "[WorkerStream] {} got {} new events, last_id: {:?} -> {:?}",
                    worker_name_clone,
                    events.len(),
                    old_last_id,
                    last_id
                );

                let responses: Vec<WorkerEventResponse> = events
                    .into_iter()
                    .map(|e| WorkerEventResponse {
                        id: e.id,
                        worker_name: e.worker_name,
                        event_type: e.event_type.as_str().to_string(),
                        timestamp: e.timestamp,
                        content: e.content,
                        tool_call_id: e.tool_call_id,
                        tool_title: e.tool_title,
                        tool_kind: e.tool_kind,
                        tool_status: e
                            .tool_status
                            .map(|s: crate::core::state::ToolCallStatus| s.as_str().to_string()),
                        tool_input: e.tool_input,
                        tool_output: e.tool_output,
                    })
                    .collect();

                if first_poll {
                    // Send all historical events as a batch
                    info!(
                        "[WorkerStream] Emitting history with {} events",
                        responses.len()
                    );
                    let event = WorkerStreamEvent::History {
                        run_name: run_name_clone.clone(),
                        worker_name: worker_name_clone.clone(),
                        events: responses,
                        worker_status: worker_status.clone(),
                    };
                    match app.emit("worker-event", &event) {
                        Ok(_) => info!("[WorkerStream] History emitted successfully"),
                        Err(e) => info!("[WorkerStream] Failed to emit history: {}", e),
                    }
                    first_poll = false;
                } else {
                    // Send individual events
                    for response in responses {
                        let event = WorkerStreamEvent::Event {
                            run_name: run_name_clone.clone(),
                            worker_name: worker_name_clone.clone(),
                            event: response,
                        };
                        if let Err(e) = app.emit("worker-event", &event) {
                            info!("[WorkerStream] Failed to emit event: {}", e);
                        }
                    }
                }
            } else if first_poll {
                // Even if no events, send empty history to indicate stream started
                info!("[WorkerStream] Emitting empty history (no events found)");
                let event = WorkerStreamEvent::History {
                    run_name: run_name_clone.clone(),
                    worker_name: worker_name_clone.clone(),
                    events: vec![],
                    worker_status: worker_status.clone(),
                };
                match app.emit("worker-event", &event) {
                    Ok(_) => info!("[WorkerStream] Empty history emitted successfully"),
                    Err(e) => info!("[WorkerStream] Failed to emit empty history: {}", e),
                }
                first_poll = false;
            }

            // Emit status event if status changed (e.g., awaiting -> working when respawned)
            let status_changed = last_status.as_ref() != worker_status.as_ref();
            if status_changed && !first_poll {
                info!(
                    "[WorkerStream] Worker {} status changed: {:?} -> {:?}",
                    worker_name_clone, last_status, worker_status
                );
                let status_event = WorkerStreamEvent::Status {
                    run_name: run_name_clone.clone(),
                    worker_name: worker_name_clone.clone(),
                    worker_status: worker_status.clone(),
                };
                let _ = app.emit("worker-event", &status_event);
            }
            last_status = worker_status.clone();

            // Check if worker is done (not actively working)
            // Continue streaming while worker is working, awaiting, or paused
            // Only end stream for error status or if worker is removed
            let is_done = worker_status
                .as_ref()
                .map(|s: &String| matches!(s.as_str(), "error"))
                .unwrap_or(false);

            if is_done && !first_poll {
                // Send status update and end stream for error status
                info!(
                    "[WorkerStream] Worker {} status is '{}', ending stream",
                    worker_name_clone,
                    worker_status.as_deref().unwrap_or("unknown")
                );
                break;
            }

            tokio::time::sleep(poll_interval).await;
        }

        // Clean up and notify stream ended
        stream_manager_clone.remove(&run_name_clone, &worker_name_clone);
        let end_event = WorkerStreamEvent::Ended {
            run_name: run_name_clone,
            worker_name: worker_name_clone,
        };
        let _ = app.emit("worker-event", &end_event);
    });

    Ok(())
}

/// Stop streaming worker events
#[tracing::instrument(skip(stream_manager))]
#[tauri::command]
pub async fn stop_worker_event_stream(
    stream_manager: tauri::State<'_, std::sync::Arc<WorkerEventStreamManager>>,
    run_name: String,
    worker_name: String,
) -> Result<(), String> {
    stream_manager.stop(&run_name, &worker_name);
    Ok(())
}
