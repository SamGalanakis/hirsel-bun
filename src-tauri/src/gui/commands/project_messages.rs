//! Project Messages Commands (Sheepfold)
//!
//! Tauri commands for project-scoped messaging.

use crate::core::{ProjectMessage, ProjectMessagesStore, ProjectThreadSummary};

/// Get messages for a project thread
#[tauri::command]
pub async fn get_project_messages(
    project_id: i64,
    thread: String,
    limit: Option<i64>,
) -> Result<Vec<ProjectMessage>, String> {
    let store = ProjectMessagesStore::open().map_err(|e| e.to_string())?;
    store
        .get_messages(project_id, &thread, limit)
        .map_err(|e| e.to_string())
}

/// Get all threads for a project with unread counts
#[tauri::command]
pub async fn get_project_threads(project_id: i64) -> Result<Vec<ProjectThreadSummary>, String> {
    let store = ProjectMessagesStore::open().map_err(|e| e.to_string())?;
    store
        .get_threads(project_id, "user")
        .map_err(|e| e.to_string())
}

/// Send a message to a project thread
#[tauri::command]
pub async fn send_project_message(
    project_id: i64,
    thread: String,
    content: String,
) -> Result<ProjectMessage, String> {
    let store = ProjectMessagesStore::open().map_err(|e| e.to_string())?;
    store
        .add_message(project_id, &thread, "user", &content, false)
        .map_err(|e| e.to_string())
}

/// Mark messages in a thread as read
#[tauri::command]
pub async fn mark_project_messages_read(project_id: i64, thread: String) -> Result<(), String> {
    let store = ProjectMessagesStore::open().map_err(|e| e.to_string())?;
    store
        .mark_messages_read(project_id, &thread, "user")
        .map_err(|e| e.to_string())
}

/// Get total unread count for a project
#[tauri::command]
pub async fn get_project_unread_count(project_id: i64) -> Result<i64, String> {
    let store = ProjectMessagesStore::open().map_err(|e| e.to_string())?;
    store
        .get_unread_count(project_id, "user")
        .map_err(|e| e.to_string())
}
