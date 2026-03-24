//! Worker concern and notification commands.

use super::types::{UnreadNotification, UnreadNotificationsResponse};
use super::ResultExt;
use crate::core::project::ProjectStore;
use crate::core::route::RouteStore;
use crate::core::WorkerConcernStore;

#[tracing::instrument]
#[tauri::command]
pub async fn get_all_unread_notifications() -> Result<UnreadNotificationsResponse, String> {
    let concern_store = WorkerConcernStore::open().await.str_err()?;
    let project_store = ProjectStore::open().await.str_err()?;
    let unread = concern_store
        .list_unread("user", Some(100))
        .await
        .str_err()?;

    let mut notifications = Vec::new();
    for concern in unread {
        let project = match project_store.get_project(concern.project_id).await {
            Ok(project) => project,
            Err(_) => continue,
        };
        let route_store = match RouteStore::new(concern.project_id).await {
            Ok(store) => store,
            Err(_) => continue,
        };
        let route = match route_store.get_route(concern.route_id).await {
            Ok(route) => route,
            Err(_) => continue,
        };

        notifications.push(UnreadNotification {
            id: format!("concern-{}", concern.id),
            concern_id: concern.id,
            project_id: concern.project_id,
            project_name: project.name,
            route_id: concern.route_id,
            route_name: route.name,
            worker_name: concern.worker_name,
            kind: concern.kind,
            severity: concern.severity,
            summary: concern.summary,
            timestamp: concern.updated_at,
        });
    }

    Ok(UnreadNotificationsResponse {
        total_runs_with_unread: notifications.len() as u32,
        notifications,
    })
}
