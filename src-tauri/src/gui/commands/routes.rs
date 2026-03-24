//! Route management commands for project exploration branches

use crate::core::route::{CreateRouteRequest, Route, RouteStore, UpdateRouteSettingsRequest};
use crate::core::route_runtime::get_route_runtime_name;
use crate::core::state::SQLiteState;

use super::ResultExt;

/// Get a route by ID
#[tracing::instrument]
#[tauri::command]
pub async fn get_route(project_id: i64, route_id: i64) -> Result<Route, String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    store.get_route(route_id).await.str_err()
}

/// Replace selected-route execution defaults.
#[tracing::instrument]
#[tauri::command]
pub async fn update_route_settings(
    project_id: i64,
    route_id: i64,
    time_limit_minutes: Option<i64>,
    human_in_the_loop: bool,
    target_branch: Option<String>,
) -> Result<Route, String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    let route = store
        .update_route_settings(
            route_id,
            &UpdateRouteSettingsRequest {
                time_limit_minutes,
                human_in_the_loop,
                target_branch,
            },
        )
        .await
        .str_err()?;

    sync_runtime_settings(project_id, route_id, &route).await?;

    Ok(route)
}

/// Create a new route (fork from parent)
#[tracing::instrument]
#[tauri::command]
pub async fn create_route(
    project_id: i64,
    name: String,
    parent_route_id: Option<i64>,
    parent_version_id: Option<i64>,
) -> Result<Route, String> {
    let store = RouteStore::new(project_id).await.str_err()?;

    let req = CreateRouteRequest {
        name,
        parent_route_id,
        parent_version_id,
    };

    store.create_route(&req).await.str_err()
}

/// Archive a route
#[tracing::instrument]
#[tauri::command]
pub async fn archive_route(project_id: i64, route_id: i64) -> Result<Route, String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    let project_store = crate::core::project::ProjectStore::open().await.str_err()?;
    let project = project_store.get_project(project_id).await.str_err()?;

    let archived = store.archive_route(route_id).await.str_err()?;

    if project.active_route_id == Some(route_id) {
        let remaining = store.list_routes().await.str_err()?;
        if let Some(next_route) = remaining.into_iter().find(|route| route.id != route_id) {
            set_active_route(project_id, next_route.id).await?;
        }
    }

    Ok(archived)
}

/// Set the active route for a project
#[tracing::instrument]
#[tauri::command]
pub async fn set_active_route(project_id: i64, route_id: i64) -> Result<(), String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    let route = store.get_route(route_id).await.str_err()?;
    if route.archived_at.is_some() {
        return Err("Cannot select an archived route".to_string());
    }

    let pool = crate::core::db::global_pool().await;
    sqlx::query("UPDATE projects SET active_route_id = ?, updated_at = ? WHERE id = ?")
        .bind(route_id)
        .bind(crate::core::db::utc_now())
        .bind(project_id)
        .execute(pool)
        .await
        .context("Failed to set active route")?;

    Ok(())
}

/// Get the active route for a project
///
/// Falls back to main route if available, otherwise first route. If no routes
/// exist yet, creates a default main route.
#[tracing::instrument]
#[tauri::command]
pub async fn get_active_route(project_id: i64) -> Result<Route, String> {
    let store = RouteStore::new(project_id).await.str_err()?;

    let project_store = crate::core::project::ProjectStore::open().await.str_err()?;
    let project = project_store.get_project(project_id).await.str_err()?;

    if let Some(route_id) = project.active_route_id {
        if let Ok(route) = store.get_route(route_id).await {
            return Ok(route);
        }
    }

    let routes = store.list_routes().await.str_err()?;
    let fallback = if let Some(route) = routes
        .iter()
        .find(|r| r.name == "main")
        .cloned()
        .or_else(|| routes.first().cloned())
    {
        route
    } else {
        store.create_main_route().await.str_err()?
    };

    let pool = crate::core::db::global_pool().await;
    let _ = sqlx::query("UPDATE projects SET active_route_id = ?, updated_at = ? WHERE id = ?")
        .bind(fallback.id)
        .bind(crate::core::db::utc_now())
        .bind(project_id)
        .execute(pool)
        .await;

    Ok(fallback)
}

async fn sync_runtime_settings(
    project_id: i64,
    route_id: i64,
    route: &Route,
) -> Result<(), String> {
    let Some(runtime_name) = get_route_runtime_name(project_id, route_id).await? else {
        return Ok(());
    };

    let state = SQLiteState::new(&runtime_name).await.str_err()?;
    state
        .set_human_in_the_loop(route.human_in_the_loop)
        .await
        .str_err()?;

    state
        .set_time_limit_minutes(route.time_limit_minutes)
        .await
        .str_err()?;

    Ok(())
}
