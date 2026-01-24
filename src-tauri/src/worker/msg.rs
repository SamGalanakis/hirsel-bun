//! Worker message commands for hirsel-worker subprocess.
//!
//! These commands are used by AI agents running inside worker tmux sessions
//! to communicate with the user and other workers via message threads.

use crate::core::state::{WorkerStatus, WorkerUpdate};
use crate::core::{Files, SQLiteState};
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
pub fn send(thread: &str, message: &str) -> MsgResult<serde_json::Value> {
    let run_dir = get_run_dir()?;
    let worker_name = get_worker_name()?;
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path())?;

    let is_user_dm = thread == "user";

    // Translate "user" thread to worker's own DM thread
    let actual_thread = if is_user_dm {
        worker_name.as_str()
    } else {
        thread
    };

    // Add the message (waiting flag set for user DMs)
    let msg_id = state.add_message(actual_thread, &worker_name, message, is_user_dm)?;

    // Only wait for reply when messaging the user
    if is_user_dm {
        let hitl_enabled = state.get_human_in_the_loop().unwrap_or(true);

        if hitl_enabled {
            // HITL mode: Set worker as waiting and optionally pause others
            state.update_worker(
                &worker_name,
                WorkerUpdate {
                    status: Some(WorkerStatus::Awaiting),
                    hitl_waiting: Some(true),
                    waiting_thread: Some(actual_thread.to_string()),
                    ..Default::default()
                },
            )?;

            // Check pause_mode to determine if we should pause all workers
            let pause_mode = state
                .get_pause_mode()
                .unwrap_or_else(|_| "sender".to_string());
            if pause_mode == "all" {
                state.pause_all_workers("Worker requested human input")?;
            }

            // Poll until hitl_waiting is cleared (by user resume action)
            let reply = poll_for_hitl_resume(&state, actual_thread, &worker_name, msg_id)?;
            Ok(json!({
                "success": true,
                "message_id": msg_id,
                "reply": reply
            }))
        } else {
            // YOLO mode: Just poll for reply without pausing
            let reply = poll_for_reply(&state, actual_thread, &worker_name, msg_id)?;
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
fn poll_for_reply(
    state: &SQLiteState,
    thread: &str,
    worker_name: &str,
    sent_msg_id: i64,
) -> MsgResult<Option<serde_json::Value>> {
    use std::thread::sleep;
    use std::time::Duration;

    let poll_interval = Duration::from_secs(2);
    let max_polls = 900; // 30 minutes max wait

    for _ in 0..max_polls {
        // Check for messages after our sent message that aren't from us
        let messages = state.get_messages(thread, 100)?;
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

        sleep(poll_interval);
    }

    // Timeout - no reply received
    Ok(None)
}

/// Poll until HITL waiting flag is cleared by user resume action
///
/// In HITL mode, workers don't automatically wake up when a reply arrives.
/// Instead, they wait until the user explicitly resumes them (clearing hitl_waiting).
/// Once resumed, this function returns any reply that was received.
fn poll_for_hitl_resume(
    state: &SQLiteState,
    thread: &str,
    worker_name: &str,
    sent_msg_id: i64,
) -> MsgResult<Option<serde_json::Value>> {
    use std::thread::sleep;
    use std::time::Duration;

    let poll_interval = Duration::from_secs(2);
    // No timeout for HITL - wait indefinitely until resumed
    // (In practice, the run may be cancelled or timed out externally)

    loop {
        // Check if hitl_waiting has been cleared (by user resume action)
        if let Ok(Some(worker)) = state.get_worker(worker_name) {
            if !worker.hitl_waiting {
                // Worker has been resumed - check for any reply
                let messages = state.get_messages(thread, 100)?;
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

        sleep(poll_interval);
    }
}

/// Read messages from a thread (or all threads if none specified)
pub fn read(thread: Option<&str>) -> MsgResult<serde_json::Value> {
    let run_dir = get_run_dir()?;
    let worker_name = get_worker_name()?;
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path())?;

    if let Some(thread_name) = thread {
        // Read from specific thread
        let messages = state.get_unread_messages(thread_name, &worker_name)?;

        // Mark as read
        if !messages.is_empty() {
            let max_id = messages.iter().map(|m| m.id).max();
            state.mark_messages_read(thread_name, &worker_name, max_id)?;
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
        let threads = state.get_threads()?;
        let mut all_messages = Vec::new();

        for thread_name in &threads {
            let messages = state.get_unread_messages(thread_name, &worker_name)?;

            if !messages.is_empty() {
                let max_id = messages.iter().map(|m| m.id).max();
                state.mark_messages_read(thread_name, &worker_name, max_id)?;

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
pub fn list() -> MsgResult<serde_json::Value> {
    let run_dir = get_run_dir()?;
    let worker_name = get_worker_name()?;
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path())?;

    let threads = state.get_threads()?;

    let mut thread_info = Vec::new();
    for thread_name in &threads {
        let message_count = state.get_thread_message_count(thread_name)?;
        let unread = state.get_unread_messages(thread_name, &worker_name)?;

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
pub fn inbox() -> MsgResult<serde_json::Value> {
    let run_dir = get_run_dir()?;
    let worker_name = get_worker_name()?;
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path())?;

    let messages = state.get_all_unread_messages(&worker_name)?;

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
    match send(thread, message) {
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
    match read(thread) {
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
    match list() {
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
    match inbox() {
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
