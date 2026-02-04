//! Route management commands for project exploration branches

use crate::core::route::{CreateRouteRequest, Route, RouteFiles, RouteStore, RouteTree};

use super::err_string;

/// List all routes for a project
#[tauri::command]
pub async fn list_routes(project_id: i64) -> Result<Vec<Route>, String> {
    let store = RouteStore::new(project_id).await.map_err(err_string)?;
    store.list_routes().await.map_err(err_string)
}

/// Get a route by ID
#[tauri::command]
pub async fn get_route(project_id: i64, route_id: i64) -> Result<Route, String> {
    let store = RouteStore::new(project_id).await.map_err(err_string)?;
    store.get_route(route_id).await.map_err(err_string)
}

/// Get a route by name
#[tauri::command]
pub async fn get_route_by_name(project_id: i64, name: String) -> Result<Option<Route>, String> {
    let store = RouteStore::new(project_id).await.map_err(err_string)?;
    store.get_route_by_name(&name).await.map_err(err_string)
}

/// Get the route tree for a project
#[tauri::command]
pub async fn get_route_tree(project_id: i64) -> Result<Vec<RouteTree>, String> {
    let store = RouteStore::new(project_id).await.map_err(err_string)?;
    store.get_route_tree().await.map_err(err_string)
}

/// Create a new route (fork from parent)
#[tauri::command]
pub async fn create_route(
    project_id: i64,
    name: String,
    parent_route_id: Option<i64>,
    parent_version_id: Option<i64>,
) -> Result<Route, String> {
    let store = RouteStore::new(project_id).await.map_err(err_string)?;

    let req = CreateRouteRequest {
        name: name.clone(),
        parent_route_id,
        parent_version_id,
    };

    let route = store.create_route(&req).await.map_err(err_string)?;

    // Initialize route files
    let route_files = RouteFiles::new(project_id, &route.name);
    route_files.init_dirs().map_err(err_string)?;

    // If forking from a parent route, copy docs and code
    if let Some(parent_id) = parent_route_id {
        let parent = store.get_route(parent_id).await.map_err(err_string)?;
        let parent_files = RouteFiles::new(project_id, &parent.name);

        if let Err(e) = route_files.copy_docs_from(&parent_files) {
            tracing::warn!("Failed to copy docs from parent route: {}", e);
        }
        if let Err(e) = route_files.copy_code_from(&parent_files) {
            tracing::warn!("Failed to copy code from parent route: {}", e);
        }
    }

    Ok(route)
}

/// Delete a route
#[tauri::command]
pub async fn delete_route(project_id: i64, route_id: i64) -> Result<(), String> {
    let store = RouteStore::new(project_id).await.map_err(err_string)?;

    // Get route name before deletion for file cleanup
    let route = store.get_route(route_id).await.map_err(err_string)?;

    // Delete from database
    store.delete_route(route_id).await.map_err(err_string)?;

    // Clean up route files
    let route_files = RouteFiles::new(project_id, &route.name);
    if let Err(e) = route_files.delete() {
        tracing::warn!("Failed to delete route directory: {}", e);
    }

    Ok(())
}

/// Set the active route for a project
#[tauri::command]
pub async fn set_active_route(project_id: i64, route_id: i64) -> Result<(), String> {
    // Verify route exists
    let store = RouteStore::new(project_id).await.map_err(err_string)?;
    let _ = store.get_route(route_id).await.map_err(err_string)?;

    // Update project's active_route_id
    let pool = crate::core::db::global_pool().await;
    sqlx::query("UPDATE projects SET active_route_id = ? WHERE id = ?")
        .bind(route_id)
        .bind(project_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to set active route: {}", e))?;

    Ok(())
}

/// Get the active route for a project
///
/// Falls back to the main route if no active route is set.
/// Creates the main route if no routes exist.
#[tauri::command]
pub async fn get_active_route(project_id: i64) -> Result<Route, String> {
    let store = RouteStore::new(project_id).await.map_err(err_string)?;

    // Get project to find active_route_id
    let project_store = crate::core::project::ProjectStore::open()
        .await
        .map_err(err_string)?;
    let project = project_store
        .get_project(project_id)
        .await
        .map_err(err_string)?;

    // If active_route_id is set, use it
    if let Some(route_id) = project.active_route_id {
        if let Ok(route) = store.get_route(route_id).await {
            return Ok(route);
        }
    }

    // Fall back to main route (create if needed)
    store.create_main_route().await.map_err(err_string)
}
