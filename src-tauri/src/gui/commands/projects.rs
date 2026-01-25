//! Project management commands
//!
//! Commands for listing, creating, and managing projects for SpecFlow boards.

use crate::core::draft::StartingPoint;
use crate::core::project::{CreateProjectRequest, Project, ProjectStore};

/// List all projects, sorted by most recently created
#[tauri::command]
pub async fn list_projects() -> Result<Vec<Project>, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    store.list_projects().map_err(|e| e.to_string())
}

/// Get a project by ID
#[tauri::command]
pub async fn get_project(project_id: i64) -> Result<Project, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    store.get_project(project_id).map_err(|e| e.to_string())
}

/// Create a new project from a local folder path
#[tauri::command]
pub async fn create_project_from_path(
    path: String,
    name: Option<String>,
) -> Result<Project, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;

    // Use folder name as project name if not provided
    let project_name = name.unwrap_or_else(|| {
        std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unnamed Project".to_string())
    });

    // Check if project with this name already exists
    if let Ok(Some(_)) = store.get_project_by_name(&project_name) {
        // Generate unique name by adding suffix
        let mut unique_name = project_name.clone();
        let mut counter = 1;
        while store
            .get_project_by_name(&unique_name)
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
            max_iterations: None,
            human_in_the_loop: None,
            docs_path: None,
            persist_docs_changes: None,
            description: None,
        };
        return store.create_project(&req).map_err(|e| e.to_string());
    }

    let req = CreateProjectRequest {
        name: project_name,
        starting_point: StartingPoint::LocalFolder { path },
        worker_scale: None,
        time_limit_minutes: None,
        max_iterations: None,
        human_in_the_loop: None,
        docs_path: None,
        persist_docs_changes: None,
        description: None,
    };

    store.create_project(&req).map_err(|e| e.to_string())
}

/// Create a new project with a name and starting point
#[tauri::command]
pub async fn create_project(
    name: String,
    starting_point: StartingPoint,
) -> Result<Project, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;

    // Check if project with this name already exists and generate unique name if needed
    let mut project_name = name;
    if let Ok(Some(_)) = store.get_project_by_name(&project_name) {
        let base_name = project_name.clone();
        let mut counter = 1;
        while store
            .get_project_by_name(&project_name)
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
        max_iterations: None,
        human_in_the_loop: None,
        docs_path: None,
        persist_docs_changes: None,
        description: None,
    };

    store.create_project(&req).map_err(|e| e.to_string())
}

/// Delete a project (removes from list, doesn't delete files)
#[tauri::command]
pub async fn delete_project(project_id: i64) -> Result<(), String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    store.delete_project(project_id).map_err(|e| e.to_string())
}
