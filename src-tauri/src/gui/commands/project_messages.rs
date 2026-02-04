//! Project Messages Commands (Sheepfold)
//!
//! Tauri commands for project-scoped messaging.
//! All commands are scoped to a specific route within a project.

use crate::core::project::ProjectStore;
use crate::core::{ProjectMessage, ProjectMessagesStore, ProjectThreadSummary};

/// Get messages for a project thread
#[tauri::command]
pub async fn get_project_messages(
    project_id: i64,
    route_id: i64,
    thread: String,
    limit: Option<i64>,
) -> Result<Vec<ProjectMessage>, String> {
    let store = ProjectMessagesStore::open()
        .await
        .map_err(|e| e.to_string())?;
    store
        .get_messages(project_id, route_id, &thread, limit)
        .await
        .map_err(|e| e.to_string())
}

/// Get all threads for a project route with unread counts
#[tauri::command]
pub async fn get_project_threads(
    project_id: i64,
    route_id: i64,
) -> Result<Vec<ProjectThreadSummary>, String> {
    let store = ProjectMessagesStore::open()
        .await
        .map_err(|e| e.to_string())?;
    store
        .get_threads(project_id, route_id, "user")
        .await
        .map_err(|e| e.to_string())
}

/// Send a message to a project thread
#[tauri::command]
pub async fn send_project_message(
    project_id: i64,
    route_id: i64,
    thread: String,
    content: String,
) -> Result<ProjectMessage, String> {
    let store = ProjectMessagesStore::open()
        .await
        .map_err(|e| e.to_string())?;
    store
        .add_message(project_id, route_id, &thread, "user", &content, false)
        .await
        .map_err(|e| e.to_string())
}

/// Mark messages in a thread as read
#[tauri::command]
pub async fn mark_project_messages_read(
    project_id: i64,
    route_id: i64,
    thread: String,
) -> Result<(), String> {
    let store = ProjectMessagesStore::open()
        .await
        .map_err(|e| e.to_string())?;
    store
        .mark_messages_read(project_id, route_id, &thread, "user")
        .await
        .map_err(|e| e.to_string())
}

/// Get total unread count for a project route
///
/// If route_id is not provided, uses the project's active_route_id.
#[tauri::command]
pub async fn get_project_unread_count(
    project_id: i64,
    route_id: Option<i64>,
) -> Result<i64, String> {
    // Get route_id from parameter or lookup from project
    let route_id = match route_id {
        Some(id) => id,
        None => {
            let project_store = ProjectStore::open().await.map_err(|e| e.to_string())?;
            let project = project_store
                .get_project(project_id)
                .await
                .map_err(|e| e.to_string())?;
            project.active_route_id.unwrap_or(1)
        }
    };

    let store = ProjectMessagesStore::open()
        .await
        .map_err(|e| e.to_string())?;
    store
        .get_unread_count(project_id, route_id, "user")
        .await
        .map_err(|e| e.to_string())
}
