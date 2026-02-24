//! Message-related commands
//!
//! Commands for unread notification aggregation from project-level messages.

use super::types::{UnreadNotification, UnreadNotificationsResponse};
use super::ResultExt;
use crate::core::api_types::parse_timestamp;
use crate::core::delta::DeltaState;
use crate::core::project::ProjectStore;
use crate::core::route::RouteStore;
use crate::core::ProjectMessagesStore;

/// Get all unread notifications across all projects
/// Uses project-level messages (Sheepfold) for notifications
#[tracing::instrument]
#[tauri::command]
pub async fn get_all_unread_notifications() -> Result<UnreadNotificationsResponse, String> {
    // Get runs from project_runs table (source of truth)
    let project_runs = DeltaState::list_all_project_runs()
        .await
        .unwrap_or_default();

    let store = ProjectMessagesStore::open().await.str_err()?;

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
            Ok(p) => match p.active_route_id {
                Some(id) => id,
                None => {
                    let store = match RouteStore::new(project_id).await {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    match store
                        .list_routes()
                        .await
                        .ok()
                        .and_then(|r| r.first().map(|x| x.id))
                    {
                        Some(id) => id,
                        None => continue,
                    }
                }
            },
            Err(_) => continue,
        };

        // Get threads with unread counts for this project
        let threads = match store.get_threads(project_id, route_id, "user").await {
            Ok(t) => t,
            Err(_) => continue,
        };

        let mut project_has_unread = false;

        for thread_info in threads {
            // Skip chat (group chat) for notifications - too noisy
            if thread_info.thread == "chat" {
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
                    project_id,
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
