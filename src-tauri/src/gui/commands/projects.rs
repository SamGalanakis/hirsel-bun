//! Project management commands
//!
//! Commands for listing, creating, and managing projects for SpecFlow boards.

use super::ResultExt;
use crate::core::config;
use crate::core::delta::DeltaState;
use crate::core::draft::StartingPoint;
use crate::core::project::{
    validate_project_focus_view_html, CreateProjectRequest, Project, ProjectFocusView,
    ProjectRetainedContext, ProjectStore, ProjectSurfaceSnapshot, RouteSummary,
    UpdateProjectRequest,
};
use crate::core::route::{CreateRouteRepoRequest, RouteStore, UpdateRouteSettingsRequest};
use crate::core::state::SQLiteState;
use tauri::Emitter;

/// List all projects, sorted by most recently created
#[tracing::instrument]
#[tauri::command]
pub async fn list_projects() -> Result<Vec<Project>, String> {
    let store = ProjectStore::open().await.str_err()?;
    store.list_projects().await.str_err()
}

/// Get a project by ID
#[tracing::instrument]
#[tauri::command]
pub async fn get_project(project_id: i64) -> Result<Project, String> {
    let store = ProjectStore::open().await.str_err()?;
    store.get_project(project_id).await.str_err()
}

#[tracing::instrument]
#[tauri::command]
pub async fn get_project_focus_view(project_id: i64) -> Result<ProjectFocusView, String> {
    let store = ProjectStore::open().await.str_err()?;
    store.get_project_focus_view(project_id).await.str_err()
}

#[tracing::instrument]
#[tauri::command]
pub async fn get_project_retained_context(
    project_id: i64,
) -> Result<ProjectRetainedContext, String> {
    let store = ProjectStore::open().await.str_err()?;
    store
        .get_project_retained_context(project_id)
        .await
        .str_err()
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
        let project_run = delta.get_project_run().await.str_err()?;
        summaries.push(RouteSummary {
            route_id: route.id,
            name: route.name,
            selected: project.active_route_id == Some(route.id),
            status: project_run
                .as_ref()
                .map(|run| run.status.as_str().to_string())
                .unwrap_or_else(|| "idle".to_string()),
            run_name: project_run.map(|run| run.run_name),
            updated_at: route.updated_at,
        });
    }

    Ok(ProjectSurfaceSnapshot {
        focus_view,
        routes: summaries,
    })
}

#[tracing::instrument(skip(app, html))]
#[tauri::command]
pub async fn update_project_focus_view(
    app: tauri::AppHandle,
    project_id: i64,
    html: String,
    source: Option<String>,
) -> Result<ProjectFocusView, String> {
    validate_project_focus_view_html(&html)?;

    let store = ProjectStore::open().await.str_err()?;
    let view = store
        .update_project_focus_view(project_id, &html, source.as_deref())
        .await
        .str_err()?;

    let _ = app.emit(
        "project-focus-view-updated",
        serde_json::json!({
            "projectId": project_id,
            "updatedAt": view.updated_at,
        }),
    );

    Ok(view)
}

#[tracing::instrument(skip(app, markdown))]
#[tauri::command]
pub async fn update_project_retained_context(
    app: tauri::AppHandle,
    project_id: i64,
    markdown: String,
    source: Option<String>,
) -> Result<ProjectRetainedContext, String> {
    let store = ProjectStore::open().await.str_err()?;
    let context = store
        .update_project_retained_context(project_id, &markdown, source.as_deref())
        .await
        .str_err()?;

    let _ = app.emit(
        "project-retained-context-updated",
        serde_json::json!({
            "projectId": project_id,
            "updatedAt": context.updated_at,
        }),
    );

    Ok(context)
}

/// Create a new project from a local folder path
#[tracing::instrument]
#[tauri::command]
pub async fn create_project_from_path(
    path: String,
    name: Option<String>,
) -> Result<Project, String> {
    let store = ProjectStore::open().await.str_err()?;

    let project_name = name.unwrap_or_else(|| {
        std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unnamed Project".to_string())
    });

    let mut final_name = project_name.clone();
    if let Ok(Some(_)) = store.get_project_by_name(&final_name).await {
        let base_name = final_name.clone();
        let mut counter = 1;
        while store
            .get_project_by_name(&final_name)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            counter += 1;
            final_name = format!("{} ({})", base_name, counter);
        }
    }

    let req = CreateProjectRequest {
        name: final_name,
        repos: vec![CreateRouteRepoRequest {
            name: Some("local".to_string()),
            starting_point: StartingPoint::LocalFolder { path },
            target_branch: Some("main".to_string()),
            runner: None,
        }],
        default_repo_index: Some(0),
        description: None,
        x: None,
        y: None,
    };

    store.create_project(&req).await.str_err()
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

/// Update a project's metadata and route-specific run configuration.
///
/// Project fields updated here: x/y, description.
/// Route fields updated here: worker_scale, time_limit_minutes, human_in_the_loop,
/// target_branch, runner.
#[tracing::instrument]
#[tauri::command]
pub async fn update_project(
    project_id: i64,
    route_id: i64,
    x: Option<f64>,
    y: Option<f64>,
    description: Option<String>,
    target_branch: Option<String>,
    worker_scale: Option<String>,
    time_limit_minutes: Option<i64>,
    human_in_the_loop: Option<bool>,
    runner: Option<String>,
) -> Result<Project, String> {
    let project_req = UpdateProjectRequest {
        name: None,
        description,
        x,
        y,
    };

    let store = ProjectStore::open().await.str_err()?;
    let project = store
        .update_project(project_id, &project_req)
        .await
        .str_err()?;

    let route_store = RouteStore::new(project_id).await.str_err()?;
    route_store
        .update_route_settings(
            route_id,
            &UpdateRouteSettingsRequest {
                worker_scale: worker_scale.clone(),
                time_limit_minutes,
                human_in_the_loop,
                target_branch,
                runner,
            },
        )
        .await
        .str_err()?;

    if worker_scale.is_some() || time_limit_minutes.is_some() || human_in_the_loop.is_some() {
        if let Err(e) = propagate_settings_to_active_run(
            project_id,
            route_id,
            &worker_scale,
            time_limit_minutes,
            human_in_the_loop,
        )
        .await
        {
            tracing::warn!(
                "Failed to propagate settings to active run for project {} route {}: {}",
                project_id,
                route_id,
                e
            );
        }
    }

    Ok(project)
}

/// Propagate route settings to the active run's state
async fn propagate_settings_to_active_run(
    project_id: i64,
    route_id: i64,
    worker_scale: &Option<String>,
    time_limit_minutes: Option<i64>,
    human_in_the_loop: Option<bool>,
) -> Result<(), String> {
    let delta_state = DeltaState::with_route(project_id, route_id);
    let project_run = delta_state.get_project_run().await.str_err()?;

    let Some(run) = project_run else {
        return Ok(());
    };

    let run_dir = config::run_dir(&run.run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Ok(());
    }

    let state = SQLiteState::new(&run.run_name).await.str_err()?;

    if let Some(scale) = worker_scale {
        state
            .set_worker_scale(scale)
            .await
            .context("Failed to set worker_scale")?;
        tracing::info!("Propagated worker_scale={} to run {}", scale, run.run_name);
    }

    if let Some(limit) = time_limit_minutes {
        state
            .set_time_limit_minutes(Some(limit))
            .await
            .context("Failed to set time_limit_minutes")?;
        tracing::info!(
            "Propagated time_limit_minutes={} to run {}",
            limit,
            run.run_name
        );
    }

    if let Some(hitl) = human_in_the_loop {
        state
            .set_human_in_the_loop(hitl)
            .await
            .context("Failed to set human_in_the_loop")?;
        tracing::info!(
            "Propagated human_in_the_loop={} to run {}",
            hitl,
            run.run_name
        );
    }

    Ok(())
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

/// Delete a project and all associated data
#[tracing::instrument]
#[tauri::command]
pub async fn delete_project(project_id: i64) -> Result<(), String> {
    let store = ProjectStore::open().await.str_err()?;
    store.delete_project(project_id).await.str_err()
}
