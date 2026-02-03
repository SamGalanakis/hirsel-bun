//! Project management commands
//!
//! Commands for listing, creating, and managing projects for SpecFlow boards.

use crate::core::config;
use crate::core::delta::DeltaState;
use crate::core::draft::StartingPoint;
use crate::core::project::{CreateProjectRequest, Project, ProjectStore, UpdateProjectRequest};
use crate::core::state::SQLiteState;

/// List all projects, sorted by most recently created
#[tauri::command]
pub async fn list_projects() -> Result<Vec<Project>, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    store.list_projects().await.map_err(|e| e.to_string())
}

/// Get a project by ID
#[tauri::command]
pub async fn get_project(project_id: i64) -> Result<Project, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    store
        .get_project(project_id)
        .await
        .map_err(|e| e.to_string())
}

/// Create a new project from a local folder path
#[tauri::command]
pub async fn create_project_from_path(
    path: String,
    name: Option<String>,
) -> Result<Project, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;

    // Use folder name as project name if not provided
    let project_name = name.unwrap_or_else(|| {
        std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unnamed Project".to_string())
    });

    // Check if project with this name already exists
    if let Ok(Some(_)) = store.get_project_by_name(&project_name).await {
        // Generate unique name by adding suffix
        let mut unique_name = project_name.clone();
        let mut counter = 1;
        while store
            .get_project_by_name(&unique_name)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            counter += 1;
            unique_name = format!("{} ({})", project_name, counter);
        }

        let req = CreateProjectRequest {
            name: unique_name,
            starting_point: StartingPoint::LocalFolder { path },
            worker_scale: None,
            time_limit_minutes: None,
            human_in_the_loop: None,
            docs_path: None,
            persist_docs_changes: None,
            description: None,
            target_branch: None,
            runner: None,
            x: None,
            y: None,
        };
        let project = store
            .create_project(&req)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(project);
    }

    let req = CreateProjectRequest {
        name: project_name,
        starting_point: StartingPoint::LocalFolder { path },
        worker_scale: None,
        time_limit_minutes: None,
        human_in_the_loop: None,
        docs_path: None,
        persist_docs_changes: None,
        description: None,
        target_branch: None,
        runner: None,
        x: None,
        y: None,
    };

    let project = store
        .create_project(&req)
        .await
        .map_err(|e| e.to_string())?;
    Ok(project)
}

/// Create a new project with a name and starting point
#[tauri::command]
pub async fn create_project(
    name: String,
    starting_point: StartingPoint,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Project, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;

    // Check if project with this name already exists and generate unique name if needed
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
        starting_point,
        worker_scale: None,
        time_limit_minutes: None,
        human_in_the_loop: None,
        docs_path: None,
        persist_docs_changes: None,
        description: None,
        target_branch: None,
        runner: None,
        x,
        y,
    };

    let project = store
        .create_project(&req)
        .await
        .map_err(|e| e.to_string())?;
    Ok(project)
}

/// Update a project's fields including run configuration
///
/// Accepts all project fields including run settings (worker_scale, time_limit_minutes,
/// human_in_the_loop, runner). When run settings change, they are
/// also propagated to any active run for this project.
#[tauri::command]
pub async fn update_project(
    project_id: i64,
    x: Option<f64>,
    y: Option<f64>,
    description: Option<String>,
    target_branch: Option<String>,
    worker_scale: Option<String>,
    time_limit_minutes: Option<i64>,
    human_in_the_loop: Option<bool>,
    runner: Option<String>,
) -> Result<Project, String> {
    let req = UpdateProjectRequest {
        name: None,
        starting_point: None,
        worker_scale: worker_scale.clone(),
        time_limit_minutes,
        human_in_the_loop,
        docs_path: None,
        persist_docs_changes: None,
        description,
        target_branch,
        runner,
        x,
        y,
    };

    // Update project
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    let project = store
        .update_project(project_id, &req)
        .await
        .map_err(|e| e.to_string())?;

    // Propagate settings to active run if one exists
    if worker_scale.is_some() || time_limit_minutes.is_some() || human_in_the_loop.is_some() {
        if let Err(e) = propagate_settings_to_active_run(project_id, &req).await {
            tracing::warn!(
                "Failed to propagate settings to active run for project {}: {}",
                project_id,
                e
            );
        }
    }

    Ok(project)
}

/// Propagate project settings to the active run's state
async fn propagate_settings_to_active_run(
    project_id: i64,
    req: &UpdateProjectRequest,
) -> Result<(), String> {
    // Find the active run for this project
    let delta_state = DeltaState::new(project_id);
    let project_run = delta_state
        .get_project_run()
        .await
        .map_err(|e| e.to_string())?;

    let Some(run) = project_run else {
        return Ok(()); // No active run
    };

    let run_dir = config::run_dir(&run.run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Ok(()); // Run doesn't have a database yet
    }

    let state = SQLiteState::new(&run.run_name)
        .await
        .map_err(|e| e.to_string())?;

    // Propagate worker_scale (scale up allows spawning more workers immediately,
    // scale down prevents spawning/waking workers beyond the new limit)
    if let Some(ref scale) = req.worker_scale {
        state
            .set_worker_scale(scale)
            .await
            .map_err(|e| format!("Failed to set worker_scale: {}", e))?;
        tracing::info!("Propagated worker_scale={} to run {}", scale, run.run_name);
    }

    // Propagate time_limit_minutes
    if let Some(limit) = req.time_limit_minutes {
        state
            .set_time_limit_minutes(Some(limit))
            .await
            .map_err(|e| format!("Failed to set time_limit_minutes: {}", e))?;
        tracing::info!(
            "Propagated time_limit_minutes={} to run {}",
            limit,
            run.run_name
        );
    }

    // Propagate human_in_the_loop
    if let Some(hitl) = req.human_in_the_loop {
        state
            .set_human_in_the_loop(hitl)
            .await
            .map_err(|e| format!("Failed to set human_in_the_loop: {}", e))?;
        tracing::info!(
            "Propagated human_in_the_loop={} to run {}",
            hitl,
            run.run_name
        );
    }

    Ok(())
}

/// Update a project's name
#[tauri::command]
pub async fn update_project_name(project_id: i64, name: String) -> Result<Project, String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;

    let req = UpdateProjectRequest {
        name: Some(name),
        starting_point: None,
        worker_scale: None,
        time_limit_minutes: None,
        human_in_the_loop: None,
        docs_path: None,
        persist_docs_changes: None,
        description: None,
        target_branch: None,
        runner: None,
        x: None,
        y: None,
    };

    store
        .update_project(project_id, &req)
        .await
        .map_err(|e| e.to_string())
}

/// Delete a project (removes from list, doesn't delete files)
#[tauri::command]
pub async fn delete_project(project_id: i64) -> Result<(), String> {
    let store = ProjectStore::open().await.map_err(|e| e.to_string())?;
    store
        .delete_project(project_id)
        .await
        .map_err(|e| e.to_string())
}
