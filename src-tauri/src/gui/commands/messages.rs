//! Message-related commands
//!
//! Commands for managing messages: getting messages, threads, sending, and marking as read.

use super::helpers::parse_timestamp;
use super::types::{Message, ThreadSummary, UnreadNotification, UnreadNotificationsResponse};
use crate::core::{config, state::SQLiteState};

/// Get messages for a thread
#[tauri::command]
pub async fn get_messages(
    run_name: String,
    thread_name: String,
    limit: Option<u32>,
) -> Result<Vec<Message>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let limit = limit.unwrap_or(100) as i64;
    let core_messages = state
        .get_messages(&thread_name, limit)
        .map_err(|e| format!("Failed to get messages: {}", e))?;

    let messages = core_messages
        .into_iter()
        .map(|m| Message {
            id: m.id as u32,
            thread: m.thread,
            sender: m.sender,
            content: m.content,
            waiting: m.waiting,
            read_by: None,
            timestamp: m.timestamp,
        })
        .collect();

    Ok(messages)
}

/// Get all threads for a run
#[tauri::command]
pub async fn get_threads(run_name: String) -> Result<Vec<ThreadSummary>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let thread_names = state
        .get_threads()
        .map_err(|e| format!("Failed to get threads: {}", e))?;

    let mut threads = Vec::new();
    for name in thread_names {
        let message_count = state.get_thread_message_count(&name).unwrap_or(0) as u32;
        let messages = state.get_messages(&name, 1).unwrap_or_default();
        let last_message = messages.first().map(|m| m.content.clone());
        let last_timestamp = messages.first().map(|m| m.timestamp.clone());

        threads.push(ThreadSummary {
            name,
            message_count,
            unread_count: 0,
            last_message,
            last_timestamp,
        });
    }

    Ok(threads)
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
                // Skip user messages (they're not notifications)
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
#[tauri::command]
pub async fn send_message(
    run_name: String,
    thread_name: String,
    content: String,
) -> Result<Message, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Add the message (user messages are not waiting)
    let message_id = state
        .add_message(&thread_name, "user", &content, false)
        .map_err(|e| format!("Failed to send message: {}", e))?;

    // Return the created message
    Ok(Message {
        id: message_id as u32,
        thread: thread_name,
        sender: "user".to_string(),
        content,
        waiting: false,
        read_by: None,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
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
