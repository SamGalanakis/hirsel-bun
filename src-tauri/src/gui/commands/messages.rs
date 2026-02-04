//! Message-related commands
//!
//! Commands for managing messages: getting messages, threads, sending, and marking as read.
//!
//! Note: Run-level messages (orchestrator) are kept for backward compatibility,
//! but workers now use project-level messages (Sheepfold) for communication.

use super::types::{UnreadNotification, UnreadNotificationsResponse};
use crate::core::api_types::{parse_timestamp, Message, ThreadSummary};
use crate::core::delta::DeltaState;
use crate::core::orchestrator::create_orchestrator;
use crate::core::project::ProjectStore;
use crate::core::ProjectMessagesStore;

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

/// Get all unread notifications across all projects
/// Uses project-level messages (Sheepfold) for notifications
#[tauri::command]
pub async fn get_all_unread_notifications() -> Result<UnreadNotificationsResponse, String> {
    // Get runs from project_runs table (source of truth)
    let project_runs = DeltaState::list_all_project_runs()
        .await
        .unwrap_or_default();

    let store = ProjectMessagesStore::open()
        .await
        .map_err(|e| e.to_string())?;

    let mut all_notifications: Vec<UnreadNotification> = Vec::new();
    let mut projects_with_unread = 0;

    // Collect unique project_ids from active runs
    let mut seen_project_ids = std::collections::HashSet::new();

    // Get project store to look up active_route_id
    let project_store = match ProjectStore::open().await {
        Ok(store) => store,
        Err(_) => {
            // If we can't open the store, return empty notifications
            return Ok(UnreadNotificationsResponse {
                notifications: vec![],
                total_runs_with_unread: 0,
            });
        }
    };

    for (project_run, _project_name) in project_runs {
        let project_id = project_run.project_id;

        // Skip if we've already processed this project
        if !seen_project_ids.insert(project_id) {
            continue;
        }

        // Get project to find active_route_id
        let route_id = match project_store.get_project(project_id).await {
            Ok(p) => p.active_route_id.unwrap_or(1), // Default to route 1 if not set
            Err(_) => continue,
        };

        // Get threads with unread counts for this project
        let threads = match store.get_threads(project_id, route_id, "user").await {
            Ok(t) => t,
            Err(_) => continue,
        };

        let mut project_has_unread = false;

        for thread_info in threads {
            // Skip meadow (group chat) for notifications - too noisy
            if thread_info.thread == "meadow" {
                continue;
            }

            if thread_info.unread_count == 0 {
                continue;
            }

            project_has_unread = true;

            // Get the unread messages
            let messages = match store
                .get_messages(
                    project_id,
                    route_id,
                    &thread_info.thread,
                    Some(thread_info.unread_count),
                )
                .await
            {
                Ok(m) => m,
                Err(_) => continue,
            };

            for msg in messages {
                // Skip messages from the user themselves or system senders
                if msg.sender == "user" || msg.sender == "admin" || msg.sender == "system" {
                    continue;
                }

                all_notifications.push(UnreadNotification {
                    id: format!("{}-{}-{}", project_run.run_name, msg.thread, msg.id),
                    run_name: project_run.run_name.clone(),
                    thread: msg.thread,
                    sender: msg.sender,
                    content: msg.content,
                    timestamp: msg.timestamp,
                });
            }
        }

        if project_has_unread {
            projects_with_unread += 1;
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
        total_runs_with_unread: projects_with_unread,
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
/// This uses project-level messages (Sheepfold)
#[tauri::command]
pub async fn mark_messages_read(
    run_name: String,
    thread_name: String,
    _reader: String,
) -> Result<(), String> {
    // Get project_id from run
    let project_runs = DeltaState::list_all_project_runs()
        .await
        .map_err(|e| e.to_string())?;

    let project_id = project_runs
        .iter()
        .find(|(pr, _)| pr.run_name == run_name)
        .map(|(pr, _)| pr.project_id)
        .ok_or_else(|| format!("Run '{}' not linked to a project", run_name))?;

    // Get project to find active_route_id
    let project_store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    let project = project_store
        .get_project(project_id)
        .await
        .map_err(|e| e.to_string())?;
    let route_id = project.active_route_id.unwrap_or(1);

    let store = ProjectMessagesStore::open()
        .await
        .map_err(|e| e.to_string())?;

    store
        .mark_messages_read(project_id, route_id, &thread_name, "user")
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}
