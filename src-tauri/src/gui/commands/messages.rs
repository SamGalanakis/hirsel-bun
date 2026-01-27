//! Message-related commands
//!
//! Commands for managing messages: getting messages, threads, sending, and marking as read.

use super::types::{UnreadNotification, UnreadNotificationsResponse};
use crate::core::api_types::{parse_timestamp, Message, ThreadSummary};
use crate::core::orchestrator::create_orchestrator;
use crate::core::{config, state::SQLiteState};

/// Get messages for a thread
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn get_messages(
    run_name: String,
    thread_name: String,
    _limit: Option<u32>,
) -> Result<Vec<Message>, String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.get_messages(&run_name, &thread_name)
        .await
        .map_err(|e| e.to_string())
}

/// Get all threads for a run
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn get_threads(run_name: String) -> Result<Vec<ThreadSummary>, String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.list_threads(&run_name)
        .await
        .map_err(|e| e.to_string())
}

/// Get all unread notifications across all runs
/// This is a single query replacement for the N+1 query pattern
#[tauri::command]
pub async fn get_all_unread_notifications() -> Result<UnreadNotificationsResponse, String> {
    // Get all run directories
    let run_names = config::list_runs().unwrap_or_default();

    let mut all_notifications: Vec<UnreadNotification> = Vec::new();
    let mut runs_with_unread = 0;

    for run_name in run_names {
        let db_path = config::run_dir(&run_name).join("hirsel.db");
        if !db_path.exists() {
            continue;
        }

        let state = match SQLiteState::new(db_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Get all unread messages for the user in this run
        let unread_messages = match state.get_all_unread_messages("user") {
            Ok(m) => m,
            Err(_) => continue,
        };

        if !unread_messages.is_empty() {
            runs_with_unread += 1;

            for msg in unread_messages {
                // Notify for DM threads (worker→human messages)
                // Skip: group chat and system senders
                // Worker DM threads are named after the worker (e.g., "willow-coopworth")
                if msg.thread == "group" {
                    continue;
                }

                // Skip messages from the user themselves or system senders
                if msg.sender == "user" || msg.sender == "admin" || msg.sender == "system" {
                    continue;
                }

                all_notifications.push(UnreadNotification {
                    id: format!("{}-{}-{}", run_name, msg.thread, msg.id),
                    run_name: run_name.clone(),
                    thread: msg.thread,
                    sender: msg.sender,
                    content: msg.content,
                    timestamp: msg.timestamp,
                });
            }
        }
    }

    // Sort by timestamp, newest first
    all_notifications.sort_by(|a, b| {
        let a_time = parse_timestamp(&a.timestamp);
        let b_time = parse_timestamp(&b.timestamp);
        match (b_time, a_time) {
            (Some(bt), Some(at)) => bt.cmp(&at),
            _ => std::cmp::Ordering::Equal,
        }
    });

    // Limit to 100 most recent (frontend also caps at 100)
    all_notifications.truncate(100);

    Ok(UnreadNotificationsResponse {
        notifications: all_notifications,
        total_runs_with_unread: runs_with_unread,
    })
}

/// Send a message to a thread
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn send_message(
    run_name: String,
    thread_name: String,
    content: String,
) -> Result<Message, String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.send_message(&run_name, &thread_name, &content)
        .await
        .map_err(|e| e.to_string())
}

/// Mark messages as read
#[tauri::command]
pub async fn mark_messages_read(
    run_name: String,
    thread_name: String,
    reader: String,
) -> Result<(), String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Mark all messages in thread as read by this reader
    state
        .mark_messages_read(&thread_name, &reader, None)
        .map_err(|e| format!("Failed to mark messages read: {}", e))?;

    Ok(())
}
