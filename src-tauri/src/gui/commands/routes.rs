//! Route management commands for project exploration branches

use crate::core::draft::StartingPoint;
use crate::core::route::{
    CreateRouteRepoRequest, CreateRouteRequest, Route, RouteRepo, RouteStore, RouteTree,
    UpdateRouteRepoRequest,
};

use super::ResultExt;

/// List all routes for a project
#[tracing::instrument]
#[tauri::command]
pub async fn list_routes(project_id: i64) -> Result<Vec<Route>, String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    store.list_routes().await.str_err()
}

/// List archived routes for a project
#[tracing::instrument]
#[tauri::command]
pub async fn list_archived_routes(project_id: i64) -> Result<Vec<Route>, String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    store.list_archived_routes().await.str_err()
}

/// Get a route by ID
#[tracing::instrument]
#[tauri::command]
pub async fn get_route(project_id: i64, route_id: i64) -> Result<Route, String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    store.get_route(route_id).await.str_err()
}

/// Get a route by name
#[tracing::instrument]
#[tauri::command]
pub async fn get_route_by_name(project_id: i64, name: String) -> Result<Option<Route>, String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    store.get_route_by_name(&name).await.str_err()
}

/// Get the route tree for a project
#[tracing::instrument]
#[tauri::command]
pub async fn get_route_tree(project_id: i64) -> Result<Vec<RouteTree>, String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    store.get_route_tree().await.str_err()
}

/// List repos linked to a route
#[tracing::instrument]
#[tauri::command]
pub async fn list_route_repos(project_id: i64, route_id: i64) -> Result<Vec<RouteRepo>, String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    store.list_route_repos(route_id).await.str_err()
}

/// Add a linked repo to a route
#[tracing::instrument]
#[tauri::command]
pub async fn create_route_repo(
    project_id: i64,
    route_id: i64,
    name: Option<String>,
    starting_point: StartingPoint,
    target_branch: Option<String>,
    runner: Option<String>,
) -> Result<RouteRepo, String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    store
        .create_route_repo(
            route_id,
            &CreateRouteRepoRequest {
                name,
                starting_point,
                target_branch,
                runner,
            },
        )
        .await
        .str_err()
}

/// Update a linked route repo
#[tracing::instrument]
#[tauri::command]
pub async fn update_route_repo(
    project_id: i64,
    route_id: i64,
    repo_id: i64,
    name: Option<String>,
    starting_point: Option<StartingPoint>,
    target_branch: Option<String>,
    runner: Option<String>,
    is_archived: Option<bool>,
) -> Result<RouteRepo, String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    store
        .update_route_repo(
            route_id,
            repo_id,
            &UpdateRouteRepoRequest {
                name,
                starting_point,
                target_branch,
                runner,
                is_archived,
            },
        )
        .await
        .str_err()
}

/// Archive a route repo
#[tracing::instrument]
#[tauri::command]
pub async fn delete_route_repo(project_id: i64, route_id: i64, repo_id: i64) -> Result<(), String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    store.delete_route_repo(route_id, repo_id).await.str_err()
}

/// Set default repo for route execution/delivery context
#[tracing::instrument]
#[tauri::command]
pub async fn set_default_route_repo(
    project_id: i64,
    route_id: i64,
    repo_id: i64,
) -> Result<Route, String> {
    let store = RouteStore::new(project_id).await.str_err()?;
    store
        .set_default_route_repo(route_id, repo_id)
        .await
        .str_err()
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
