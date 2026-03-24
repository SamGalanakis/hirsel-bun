//! Project management commands
//!
//! Commands for listing, creating, and managing projects for SpecFlow boards.

use super::ResultExt;
use crate::core::delta::DeltaState;
use crate::core::project::{
    CreateProjectRequest, Project, ProjectStore, ProjectSurfaceSnapshot, RouteSummary,
    UpdateProjectRequest,
};
use crate::core::route::{CreateRouteRepoRequest, RouteStore};

/// List all projects, sorted by most recently created
#[tracing::instrument]
#[tauri::command]
pub async fn list_projects() -> Result<Vec<Project>, String> {
    let store = ProjectStore::open().await.str_err()?;
    store.list_projects().await.str_err()
}

#[tracing::instrument]
#[tauri::command]
pub async fn get_project_surface(project_id: i64) -> Result<ProjectSurfaceSnapshot, String> {
    let store = ProjectStore::open().await.str_err()?;
    let project = store.get_project(project_id).await.str_err()?;
    let focus_view = store.get_project_focus_view(project_id).await.str_err()?;

    let route_store = RouteStore::new(project_id).await.str_err()?;
    let routes = route_store.list_routes().await.str_err()?;
    let mut summaries = Vec::with_capacity(routes.len());

    for route in routes {
        let delta = DeltaState::with_route(project_id, route.id);
        let project_run = delta.get_route_runtime().await.str_err()?;
        summaries.push(RouteSummary {
            route_id: route.id,
            name: route.name,
            selected: project.active_route_id == Some(route.id),
            status: project_run
                .as_ref()
                .map(|run| run.status.as_str().to_string())
                .unwrap_or_else(|| "idle".to_string()),
            updated_at: route.updated_at,
        });
    }

    Ok(ProjectSurfaceSnapshot {
        focus_view,
        routes: summaries,
    })
}

/// Create a new project with one or more linked repos
#[tracing::instrument]
#[tauri::command]
pub async fn create_project(
    name: String,
    repos: Vec<CreateRouteRepoRequest>,
    default_repo_index: Option<usize>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Project, String> {
    let store = ProjectStore::open().await.str_err()?;

    let mut project_name = name;
    if let Ok(Some(_)) = store.get_project_by_name(&project_name).await {
        let base_name = project_name.clone();
        let mut counter = 1;
        while store
            .get_project_by_name(&project_name)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            counter += 1;
            project_name = format!("{} ({})", base_name, counter);
        }
    }

    let req = CreateProjectRequest {
        name: project_name,
        repos,
        default_repo_index,
        description: None,
        x,
        y,
    };

    let project = store.create_project(&req).await.str_err()?;
    Ok(project)
}

/// Replace the project description.
#[tracing::instrument]
#[tauri::command]
pub async fn update_project_description(
    project_id: i64,
    description: Option<String>,
) -> Result<Project, String> {
    let store = ProjectStore::open().await.str_err()?;
    store
        .set_project_description(project_id, description.as_deref())
        .await
        .str_err()
}

/// Update a project's name
#[tracing::instrument]
#[tauri::command]
pub async fn update_project_name(project_id: i64, name: String) -> Result<Project, String> {
    let store = ProjectStore::open().await.str_err()?;

    let req = UpdateProjectRequest {
        name: Some(name),
        description: None,
        x: None,
        y: None,
    };

    store.update_project(project_id, &req).await.str_err()
}
