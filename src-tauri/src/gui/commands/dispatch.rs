//! Dispatch commands
//!
//! Commands for dispatching runs from board tasks.

use crate::core::board::{BoardSnapshot, DispatchPreview, TaskRun};
use crate::core::dispatch::{DispatchConfig, DispatchInfo, DispatchService};
use crate::core::ProjectStore;

/// Preview what will be dispatched from a task
#[tauri::command]
pub async fn preview_dispatch(project_id: i64, task_id: String) -> Result<DispatchPreview, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let service = DispatchService::new(project_id);
    service
        .preview_dispatch(&task_id)
        .map_err(|e| e.to_string())
}

/// Prepare a dispatch (generates spec/eval content) without creating the run
#[tauri::command]
pub async fn prepare_dispatch(
    project_id: i64,
    task_id: String,
    run_name: Option<String>,
    target_branch: Option<String>,
    worker_scale: Option<String>,
    time_limit_minutes: Option<i64>,
) -> Result<DispatchInfo, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let config = DispatchConfig {
        run_name,
        target_branch,
        worker_scale,
        time_limit_minutes,
    };

    let service = DispatchService::new(project_id);
    service
        .prepare_dispatch(&task_id, &config)
        .map_err(|e| e.to_string())
}

/// Record that a run was dispatched from a task
#[tauri::command]
pub async fn record_dispatch(
    project_id: i64,
    task_id: String,
    run_name: String,
) -> Result<(), String> {
    let service = DispatchService::new(project_id);
    service
        .record_dispatch(&task_id, &run_name)
        .map_err(|e| e.to_string())
}

/// Get all runs dispatched from a specific task
#[tauri::command]
pub async fn get_task_runs(project_id: i64, task_id: String) -> Result<Vec<TaskRun>, String> {
    let service = DispatchService::new(project_id);
    service.get_task_runs(&task_id).map_err(|e| e.to_string())
}

/// Get all task runs for a project
#[tauri::command]
pub async fn get_all_task_runs(project_id: i64) -> Result<Vec<TaskRun>, String> {
    let service = DispatchService::new(project_id);
    service.get_all_task_runs().map_err(|e| e.to_string())
}

/// Create a board snapshot for a dispatch
#[tauri::command]
pub async fn create_dispatch_snapshot(
    project_id: i64,
    task_ids: Vec<String>,
) -> Result<BoardSnapshot, String> {
    let service = DispatchService::new(project_id);
    service
        .create_snapshot(&task_ids)
        .map_err(|e| e.to_string())
}

/// Get the dispatch scope for multiple root tasks
#[tauri::command]
pub async fn get_multi_dispatch_scope(
    project_id: i64,
    root_task_ids: Vec<String>,
) -> Result<crate::core::dispatch::DispatchScope, String> {
    let service = DispatchService::new(project_id);
    service
        .get_multi_dispatch_scope(&root_task_ids)
        .map_err(|e| e.to_string())
}

/// Prepare a multi-root dispatch
#[tauri::command]
pub async fn prepare_multi_dispatch(
    project_id: i64,
    root_task_ids: Vec<String>,
    run_name: Option<String>,
    target_branch: Option<String>,
    worker_scale: Option<String>,
    time_limit_minutes: Option<i64>,
) -> Result<DispatchInfo, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let config = DispatchConfig {
        run_name,
        target_branch,
        worker_scale,
        time_limit_minutes,
    };

    let service = DispatchService::new(project_id);
    service
        .prepare_multi_dispatch(&root_task_ids, &config)
        .map_err(|e| e.to_string())
}
