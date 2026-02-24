//! Project Messages Commands (Sheepfold)
//!
//! Tauri commands for project-scoped messaging.
//! All commands are scoped to a specific route within a project.

use super::ResultExt;
use crate::core::project::ProjectStore;
use crate::core::route::RouteStore;
use crate::core::{ProjectMessage, ProjectMessagesStore, ProjectThreadSummary};

/// Get messages for a project thread
#[tracing::instrument]
#[tauri::command]
pub async fn get_project_messages(
    project_id: i64,
    route_id: i64,
    thread: String,
    limit: Option<i64>,
) -> Result<Vec<ProjectMessage>, String> {
    let store = ProjectMessagesStore::open().await.str_err()?;
    store
        .get_messages(project_id, route_id, &thread, limit)
        .await
        .str_err()
}

/// Get all threads for a project route with unread counts
#[tracing::instrument]
#[tauri::command]
pub async fn get_project_threads(
    project_id: i64,
    route_id: i64,
) -> Result<Vec<ProjectThreadSummary>, String> {
    let store = ProjectMessagesStore::open().await.str_err()?;
    store
        .get_threads(project_id, route_id, "user")
        .await
        .str_err()
}

/// Send a message to a project thread
#[tracing::instrument(skip(content))]
#[tauri::command]
pub async fn send_project_message(
    project_id: i64,
    route_id: i64,
    thread: String,
    content: String,
) -> Result<ProjectMessage, String> {
    let store = ProjectMessagesStore::open().await.str_err()?;
    store
        .add_message(project_id, route_id, &thread, "user", &content, false)
        .await
        .str_err()
}

/// Mark messages in a thread as read
#[tracing::instrument]
#[tauri::command]
pub async fn mark_project_messages_read(
    project_id: i64,
    route_id: Option<i64>,
    thread: String,
) -> Result<(), String> {
    let route_id = match route_id {
        Some(id) => id,
        None => {
            let project_store = ProjectStore::open().await.str_err()?;
            let project = project_store.get_project(project_id).await.str_err()?;
            if let Some(id) = project.active_route_id {
                id
            } else {
                let route_store = RouteStore::new(project_id).await.str_err()?;
                route_store
                    .list_routes()
                    .await
                    .str_err()?
                    .first()
                    .map(|r| r.id)
                    .ok_or_else(|| "No routes found for project".to_string())?
            }
        }
    };

    let store = ProjectMessagesStore::open().await.str_err()?;
    store
        .mark_messages_read(project_id, route_id, &thread, "user")
        .await
        .str_err()
}

/// Get total unread count for a project route
///
/// If route_id is not provided, uses the project's active_route_id.
#[tracing::instrument]
#[tauri::command]
pub async fn get_project_unread_count(
    project_id: i64,
    route_id: Option<i64>,
) -> Result<i64, String> {
    // Get route_id from parameter or lookup from project
    let route_id = match route_id {
        Some(id) => id,
        None => {
            let project_store = ProjectStore::open().await.str_err()?;
            let project = project_store.get_project(project_id).await.str_err()?;
            if let Some(id) = project.active_route_id {
                id
            } else {
                let route_store = RouteStore::new(project_id).await.str_err()?;
                route_store
                    .list_routes()
                    .await
                    .str_err()?
                    .first()
                    .map(|r| r.id)
                    .ok_or_else(|| "No routes found for project".to_string())?
            }
        }
    };

    let store = ProjectMessagesStore::open().await.str_err()?;
    store
        .get_unread_count(project_id, route_id, "user")
        .await
        .str_err()
}
