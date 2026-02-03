//! Worker message commands for hirsel-worker subprocess.
//!
//! These commands are used by AI agents running inside worker tmux sessions
//! to communicate with the user and other workers via message threads.

use crate::core::state::{WorkerStatus, WorkerUpdate};
use crate::core::SQLiteState;
use serde_json::json;
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur in worker message operations
#[derive(Debug, Error)]
pub enum MsgError {
    #[error("Run directory not found: {0}")]
    RunDirNotFound(PathBuf),

    #[error("Database error: {0}")]
    Database(#[from] crate::core::state::StateError),

    #[error("Worker name not set (HIRSEL_WORKER_NAME env var missing)")]
    NoWorkerName,

    #[error("Thread not found: {0}")]
    ThreadNotFound(String),
}

pub type MsgResult<T> = Result<T, MsgError>;

/// Get the run directory from environment
fn get_run_dir() -> MsgResult<PathBuf> {
    let run_dir = std::env::var("HIRSEL_RUN_DIR")
        .map(PathBuf::from)
        .map_err(|_| MsgError::RunDirNotFound(PathBuf::from("(HIRSEL_RUN_DIR not set)")))?;

    if !run_dir.exists() {
        return Err(MsgError::RunDirNotFound(run_dir));
    }

    Ok(run_dir)
}

/// Extract run_name from run_dir path (last component)
fn get_run_name(run_dir: &std::path::Path) -> MsgResult<String> {
    run_dir
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .ok_or_else(|| MsgError::RunDirNotFound(run_dir.to_path_buf()))
}

/// Get the worker name from environment
fn get_worker_name() -> MsgResult<String> {
    std::env::var("HIRSEL_WORKER_NAME").map_err(|_| MsgError::NoWorkerName)
}

/// Send a message to a thread
///
/// When sending to the "user" thread with HITL mode enabled:
/// - The worker is marked as waiting for user input
/// - Based on pause_mode, either just this worker or all workers are paused
/// - The function blocks until the user resumes the worker(s)
///
/// When sending to "user" with HITL disabled (YOLO mode):
/// - The function polls for a reply and returns immediately when one arrives
///
/// For other threads (group chat), messages are sent without waiting.
pub async fn send(thread: &str, message: &str) -> MsgResult<serde_json::Value> {
    let run_dir = get_run_dir()?;
    let run_name = get_run_name(&run_dir)?;
    let worker_name = get_worker_name()?;
    let state = SQLiteState::new(&run_name).await?;

    let is_user_dm = thread == "user";

    // Translate "user" thread to worker's own DM thread
    let actual_thread = if is_user_dm {
        worker_name.as_str()
    } else {
        thread
    };

    // Add the message (waiting flag set for user DMs)
    let msg_id = state
        .add_message(actual_thread, &worker_name, message, is_user_dm)
        .await?;

    // Only wait for reply when messaging the user
    if is_user_dm {
        let hitl_enabled = state.get_human_in_the_loop().await.unwrap_or(true);

        if hitl_enabled {
            // HITL mode: Set worker as waiting and optionally pause others
            state
                .update_worker(
                    &worker_name,
                    WorkerUpdate {
                        status: Some(WorkerStatus::Awaiting),
                        hitl_waiting: Some(true),
                        waiting_thread: Some(actual_thread.to_string()),
                        ..Default::default()
                    },
                )
                .await?;

            // Check pause_mode to determine if we should pause all workers
            let pause_mode = state
                .get_pause_mode()
                .await
                .unwrap_or_else(|_| "sender".to_string());
            if pause_mode == "all" {
                state
                    .pause_all_workers("Worker requested human input")
                    .await?;
            }

            // Poll until hitl_waiting is cleared (by user resume action)
            let reply = poll_for_hitl_resume(&state, actual_thread, &worker_name, msg_id).await?;
            Ok(json!({
                "success": true,
                "message_id": msg_id,
                "reply": reply
            }))
        } else {
            // YOLO mode: Just poll for reply without pausing
            let reply = poll_for_reply(&state, actual_thread, &worker_name, msg_id).await?;
            Ok(json!({
                "success": true,
                "message_id": msg_id,
                "reply": reply
            }))
        }
    } else {
        Ok(json!({
            "success": true,
            "message_id": msg_id
        }))
    }
}

/// Poll for a reply to a waiting message (YOLO mode - no HITL)
async fn poll_for_reply(
    state: &SQLiteState,
    thread: &str,
    worker_name: &str,
    sent_msg_id: i64,
) -> MsgResult<Option<serde_json::Value>> {
    use std::time::Duration;
    use tokio::time::sleep;

    let poll_interval = Duration::from_secs(2);
    let max_polls = 900; // 30 minutes max wait

    for _ in 0..max_polls {
        // Check for messages after our sent message that aren't from us
        let messages = state.get_messages(thread, 100).await?;
        for msg in messages {
            if msg.id > sent_msg_id && msg.sender != worker_name {
                // Found a reply
                return Ok(Some(json!({
                    "id": msg.id,
                    "sender": msg.sender,
                    "content": msg.content,
                    "timestamp": msg.timestamp
                })));
            }
        }

        sleep(poll_interval).await;
    }

    // Timeout - no reply received
    Ok(None)
}

/// Poll until HITL waiting flag is cleared by user resume action
///
/// In HITL mode, workers don't automatically wake up when a reply arrives.
/// Instead, they wait until the user explicitly resumes them (clearing hitl_waiting).
/// Once resumed, this function returns any reply that was received.
async fn poll_for_hitl_resume(
    state: &SQLiteState,
    thread: &str,
    worker_name: &str,
    sent_msg_id: i64,
) -> MsgResult<Option<serde_json::Value>> {
    use std::time::Duration;
    use tokio::time::sleep;

    let poll_interval = Duration::from_secs(2);
    // No timeout for HITL - wait indefinitely until resumed
    // (In practice, the run may be cancelled or timed out externally)

    loop {
        // Check if hitl_waiting has been cleared (by user resume action)
        if let Ok(Some(worker)) = state.get_worker(worker_name).await {
            if !worker.hitl_waiting {
                // Worker has been resumed - check for any reply
                let messages = state.get_messages(thread, 100).await?;
                for msg in messages {
                    if msg.id > sent_msg_id && msg.sender != worker_name {
                        // Found a reply
                        return Ok(Some(json!({
                            "id": msg.id,
                            "sender": msg.sender,
                            "content": msg.content,
                            "timestamp": msg.timestamp
                        })));
                    }
                }
                // Resumed but no reply yet - return None
                return Ok(None);
            }
        }

        sleep(poll_interval).await;
    }
}

/// Read messages from a thread (or all threads if none specified)
pub async fn read(thread: Option<&str>) -> MsgResult<serde_json::Value> {
    let run_dir = get_run_dir()?;
    let run_name = get_run_name(&run_dir)?;
    let worker_name = get_worker_name()?;
    let state = SQLiteState::new(&run_name).await?;

    if let Some(thread_name) = thread {
        // Read from specific thread
        let messages = state.get_unread_messages(thread_name, &worker_name).await?;

        // Mark as read
        if !messages.is_empty() {
            let max_id = messages.iter().map(|m| m.id).max();
            state
                .mark_messages_read(thread_name, &worker_name, max_id)
                .await?;
        }

        let formatted: Vec<_> = messages
            .iter()
            .map(|m| {
                json!({
                    "id": m.id,
                    "sender": m.sender,
                    "content": m.content,
                    "timestamp": m.timestamp,
                    "waiting": m.waiting
                })
            })
            .collect();

        Ok(json!({
            "success": true,
            "thread": thread_name,
            "messages": formatted,
            "count": formatted.len()
        }))
    } else {
        // Read from all threads
        let threads = state.get_threads().await?;
        let mut all_messages = Vec::new();

        for thread_name in &threads {
            let messages = state.get_unread_messages(thread_name, &worker_name).await?;

            if !messages.is_empty() {
                let max_id = messages.iter().map(|m| m.id).max();
                state
                    .mark_messages_read(thread_name, &worker_name, max_id)
                    .await?;

                for m in messages {
                    all_messages.push(json!({
                        "thread": thread_name,
                        "id": m.id,
                        "sender": m.sender,
                        "content": m.content,
                        "timestamp": m.timestamp,
                        "waiting": m.waiting
                    }));
                }
            }
        }

        Ok(json!({
            "success": true,
            "messages": all_messages,
            "count": all_messages.len()
        }))
    }
}

/// List available message threads
pub async fn list() -> MsgResult<serde_json::Value> {
    let run_dir = get_run_dir()?;
    let run_name = get_run_name(&run_dir)?;
    let worker_name = get_worker_name()?;
    let state = SQLiteState::new(&run_name).await?;

    let threads = state.get_threads().await?;

    let mut thread_info = Vec::new();
    for thread_name in &threads {
        let message_count = state.get_thread_message_count(thread_name).await?;
        let unread = state.get_unread_messages(thread_name, &worker_name).await?;

        thread_info.push(json!({
            "name": thread_name,
            "message_count": message_count,
            "unread_count": unread.len()
        }));
    }

    Ok(json!({
        "success": true,
        "threads": thread_info
    }))
}

/// Check inbox for new messages since session started
///
/// Returns unread messages from all threads without marking them as read.
pub async fn inbox() -> MsgResult<serde_json::Value> {
    let run_dir = get_run_dir()?;
    let run_name = get_run_name(&run_dir)?;
    let worker_name = get_worker_name()?;
    let state = SQLiteState::new(&run_name).await?;

    let messages = state.get_all_unread_messages(&worker_name).await?;

    let formatted: Vec<_> = messages
        .iter()
        .map(|m| {
            json!({
                "thread": m.thread,
                "id": m.id,
                "sender": m.sender,
                "content": m.content,
                "timestamp": m.timestamp,
                "waiting": m.waiting
            })
        })
        .collect();

    Ok(json!({
        "success": true,
        "unread_count": formatted.len(),
        "messages": formatted
    }))
}

/// Execute a worker message command and print JSON output
pub fn execute_send(thread: &str, message: &str) {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    match rt.block_on(send(thread, message)) {
        Ok(result) => println!("{}", serde_json::to_string_pretty(&result).unwrap()),
        Err(e) => {
            let error = json!({
                "success": false,
                "error": e.to_string()
            });
            eprintln!("{}", serde_json::to_string_pretty(&error).unwrap());
            std::process::exit(1);
        }
    }
}

/// Execute a worker message read command and print JSON output
pub fn execute_read(thread: Option<&str>) {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    match rt.block_on(read(thread)) {
        Ok(result) => println!("{}", serde_json::to_string_pretty(&result).unwrap()),
        Err(e) => {
            let error = json!({
                "success": false,
                "error": e.to_string()
            });
            eprintln!("{}", serde_json::to_string_pretty(&error).unwrap());
            std::process::exit(1);
        }
    }
}

/// Execute a worker message list command and print JSON output
pub fn execute_list() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    match rt.block_on(list()) {
        Ok(result) => println!("{}", serde_json::to_string_pretty(&result).unwrap()),
        Err(e) => {
            let error = json!({
                "success": false,
                "error": e.to_string()
            });
            eprintln!("{}", serde_json::to_string_pretty(&error).unwrap());
            std::process::exit(1);
        }
    }
}

/// Execute a worker inbox command and print JSON output
pub fn execute_inbox() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    match rt.block_on(inbox()) {
        Ok(result) => println!("{}", serde_json::to_string_pretty(&result).unwrap()),
        Err(e) => {
            let error = json!({
                "success": false,
                "error": e.to_string()
            });
            eprintln!("{}", serde_json::to_string_pretty(&error).unwrap());
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_run_dir_not_set() {
        // Clear env var if set
        std::env::remove_var("HIRSEL_RUN_DIR");

        let result = get_run_dir();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MsgError::RunDirNotFound(_)));
    }

    #[test]
    fn test_get_worker_name_not_set() {
        // Clear env var if set
        std::env::remove_var("HIRSEL_WORKER_NAME");

        let result = get_worker_name();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MsgError::NoWorkerName));
    }
}
