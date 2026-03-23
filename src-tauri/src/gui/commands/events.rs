//! Route-scoped worker event streaming commands.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;

use crate::core::api_types::{WorkerEventResponse, WorkerEventsResponse};
use crate::core::{config, state::SQLiteState};

use super::workers::resolve_route_runtime_name;
use super::ResultExt;

pub struct WorkerEventStreamManager {
    streams: Mutex<HashMap<(String, String), oneshot::Sender<()>>>,
}

impl WorkerEventStreamManager {
    pub fn new() -> Self {
        Self {
            streams: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(&self, route_key: &str, worker_name: &str, cancel_tx: oneshot::Sender<()>) {
        let mut streams = self.streams.lock().unwrap();
        streams.insert((route_key.to_string(), worker_name.to_string()), cancel_tx);
    }

    pub fn stop(&self, route_key: &str, worker_name: &str) -> bool {
        let mut streams = self.streams.lock().unwrap();
        if let Some(cancel_tx) = streams.remove(&(route_key.to_string(), worker_name.to_string())) {
            let _ = cancel_tx.send(());
            true
        } else {
            false
        }
    }

    pub fn remove(&self, route_key: &str, worker_name: &str) {
        let mut streams = self.streams.lock().unwrap();
        streams.remove(&(route_key.to_string(), worker_name.to_string()));
    }
}

impl Default for WorkerEventStreamManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum WorkerStreamEvent {
    #[serde(rename = "history")]
    History {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "routeId")]
        route_id: i64,
        #[serde(rename = "workerName")]
        worker_name: String,
        events: Vec<WorkerEventResponse>,
        #[serde(rename = "workerStatus")]
        worker_status: Option<String>,
    },
    #[serde(rename = "event")]
    Event {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "routeId")]
        route_id: i64,
        #[serde(rename = "workerName")]
        worker_name: String,
        event: WorkerEventResponse,
    },
    #[serde(rename = "status")]
    Status {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "routeId")]
        route_id: i64,
        #[serde(rename = "workerName")]
        worker_name: String,
        #[serde(rename = "workerStatus")]
        worker_status: Option<String>,
    },
    #[serde(rename = "ended")]
    Ended {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "routeId")]
        route_id: i64,
        #[serde(rename = "workerName")]
        worker_name: String,
    },
}

fn map_worker_event(event: crate::core::state::WorkerEvent) -> WorkerEventResponse {
    WorkerEventResponse {
        id: event.id,
        worker_name: event.worker_name,
        event_type: event.event_type.as_str().to_string(),
        timestamp: event.timestamp,
        content: event.content,
        tool_call_id: event.tool_call_id,
        tool_title: event.tool_title,
        tool_kind: event.tool_kind,
        tool_status: event.tool_status.map(|status| status.as_str().to_string()),
        tool_input: event.tool_input,
        tool_output: event.tool_output,
    }
}

#[tracing::instrument]
#[tauri::command]
pub async fn get_route_worker_events(
    project_id: i64,
    route_id: i64,
    worker_name: String,
    after_id: Option<i64>,
    limit: Option<i64>,
) -> Result<WorkerEventsResponse, String> {
    let runtime_name = match resolve_route_runtime_name(project_id, route_id).await? {
        Some(runtime_name) => runtime_name,
        None => {
            return Ok(WorkerEventsResponse {
                events: Vec::new(),
                last_id: None,
                worker_status: None,
            });
        }
    };
    let state = SQLiteState::new(&runtime_name).await.str_err()?;
    let events = state
        .get_worker_events(&worker_name, after_id, limit.unwrap_or(1000))
        .await
        .context("Failed to get worker events")?;
    let worker_status = state
        .get_worker(&worker_name)
        .await
        .ok()
        .flatten()
        .map(|worker| worker.status.as_str().to_string());

    Ok(WorkerEventsResponse {
        last_id: events.last().map(|event| event.id),
        events: events.into_iter().map(map_worker_event).collect(),
        worker_status,
    })
}

#[tracing::instrument]
#[tauri::command]
pub async fn clear_route_worker_events(
    project_id: i64,
    route_id: i64,
    worker_name: String,
) -> Result<(), String> {
    let Some(runtime_name) = resolve_route_runtime_name(project_id, route_id).await? else {
        return Ok(());
    };
    let state = SQLiteState::new(&runtime_name).await.str_err()?;
    state
        .clear_worker_events(&worker_name)
        .await
        .context("Failed to clear worker events")
}

#[tracing::instrument(skip(app, stream_manager))]
#[tauri::command]
pub async fn start_route_worker_event_stream(
    app: tauri::AppHandle,
    stream_manager: tauri::State<'_, std::sync::Arc<WorkerEventStreamManager>>,
    project_id: i64,
    route_id: i64,
    worker_name: String,
) -> Result<(), String> {
    use tauri::Emitter;

    let runtime_name = resolve_route_runtime_name(project_id, route_id)
        .await?
        .ok_or_else(|| "No route runtime exists yet".to_string())?;
    let route_key = format!("{}:{}", project_id, route_id);
    let db_path = config::runtime_dir(&runtime_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!(
            "Route runtime database not found for route {}/{}",
            project_id, route_id
        ));
    }

    stream_manager.stop(&route_key, &worker_name);

    let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
    stream_manager.register(&route_key, &worker_name, cancel_tx);

    let stream_manager = stream_manager.inner().clone();
    let worker_name_clone = worker_name.clone();
    let runtime_name_clone = runtime_name.clone();
    let route_key_clone = route_key.clone();
    let app = app.clone();

    tokio::spawn(async move {
        let mut last_id: Option<i64> = None;
        let mut last_status: Option<String> = None;
        let mut first_poll = true;

        loop {
            if cancel_rx.try_recv().is_ok() {
                break;
            }

            let state = match SQLiteState::new(&runtime_name_clone).await {
                Ok(state) => state,
                Err(_) => break,
            };

            let events = match state
                .get_worker_events(&worker_name_clone, last_id, 1000)
                .await
            {
                Ok(events) => events,
                Err(_) => break,
            };
            let worker_status = state
                .get_worker(&worker_name_clone)
                .await
                .ok()
                .flatten()
                .map(|worker| worker.status.as_str().to_string());

            if !events.is_empty() {
                last_id = events.last().map(|event| event.id);
                let mapped = events.into_iter().map(map_worker_event).collect::<Vec<_>>();
                if first_poll {
                    let _ = app.emit(
                        "worker-event",
                        WorkerStreamEvent::History {
                            project_id,
                            route_id,
                            worker_name: worker_name_clone.clone(),
                            events: mapped,
                            worker_status: worker_status.clone(),
                        },
                    );
                    first_poll = false;
                } else {
                    for event in mapped {
                        let _ = app.emit(
                            "worker-event",
                            WorkerStreamEvent::Event {
                                project_id,
                                route_id,
                                worker_name: worker_name_clone.clone(),
                                event,
                            },
                        );
                    }
                }
            } else if first_poll {
                let _ = app.emit(
                    "worker-event",
                    WorkerStreamEvent::History {
                        project_id,
                        route_id,
                        worker_name: worker_name_clone.clone(),
                        events: Vec::new(),
                        worker_status: worker_status.clone(),
                    },
                );
                first_poll = false;
            }

            if last_status.as_ref() != worker_status.as_ref() && !first_poll {
                let _ = app.emit(
                    "worker-event",
                    WorkerStreamEvent::Status {
                        project_id,
                        route_id,
                        worker_name: worker_name_clone.clone(),
                        worker_status: worker_status.clone(),
                    },
                );
            }
            last_status = worker_status.clone();

            let should_end = worker_status
                .as_deref()
                .map(|status| status == "error")
                .unwrap_or(false);
            if should_end && !first_poll {
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }

        stream_manager.remove(&route_key_clone, &worker_name_clone);
        let _ = app.emit(
            "worker-event",
            WorkerStreamEvent::Ended {
                project_id,
                route_id,
                worker_name: worker_name_clone,
            },
        );
    });

    Ok(())
}

#[tracing::instrument(skip(stream_manager))]
#[tauri::command]
pub async fn stop_route_worker_event_stream(
    stream_manager: tauri::State<'_, std::sync::Arc<WorkerEventStreamManager>>,
    project_id: i64,
    route_id: i64,
    worker_name: String,
) -> Result<(), String> {
    stream_manager.stop(&format!("{}:{}", project_id, route_id), &worker_name);
    Ok(())
}
