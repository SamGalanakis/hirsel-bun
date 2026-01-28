//! SpecFlow board commands
//!
//! Commands for managing the SpecFlow board: tasks, evals, and bookmarks.

use crate::core::board::{
    BoardService, Bookmark, CreateEvalRequest, CreateTaskRequest, Eval, EvalStatus, ExportScope,
    SyncResult, Task, TaskStatus, TaskTree, UpdateEvalRequest, UpdateTaskRequest,
};
use crate::core::ProjectStore;

// ========== TASK COMMANDS ==========

/// Get all tasks for a project as a flat list
#[tauri::command]
pub async fn get_board_tasks(project_id: i64) -> Result<Vec<Task>, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let service = BoardService::new(project_id);
    service.get_tasks().map_err(|e| e.to_string())
}

/// Get the task tree for a project (with validation computed)
#[tauri::command]
pub async fn get_board_task_tree(project_id: i64) -> Result<Vec<TaskTree>, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let service = BoardService::new(project_id);
    service.get_task_tree().map_err(|e| e.to_string())
}

/// Create a new task
#[tauri::command]
pub async fn create_board_task(
    project_id: i64,
    parent_id: Option<String>,
    name: String,
    content: Option<String>,
) -> Result<Task, String> {
    let service = BoardService::new(project_id);
    service
        .create_task(&CreateTaskRequest {
            parent_id,
            name,
            content: content.unwrap_or_default(),
        })
        .map_err(|e| e.to_string())
}

/// Update a task
#[tauri::command]
pub async fn update_board_task(
    project_id: i64,
    task_id: String,
    name: Option<String>,
    status: Option<String>,
    content: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Task, String> {
    let service = BoardService::new(project_id);
    service
        .update_task(
            &task_id,
            &UpdateTaskRequest {
                name,
                status: status.map(|s| TaskStatus::from_str(&s)),
                content,
                x,
                y,
            },
        )
        .map_err(|e| e.to_string())
}

/// Delete a task (and all descendants)
#[tauri::command]
pub async fn delete_board_task(project_id: i64, task_id: String) -> Result<(), String> {
    let service = BoardService::new(project_id);
    service.delete_task(&task_id).map_err(|e| e.to_string())
}

/// Move a task to a new parent and/or position
#[tauri::command]
pub async fn move_board_task(
    project_id: i64,
    task_id: String,
    new_parent_id: Option<String>,
    new_position: i32,
) -> Result<(), String> {
    let service = BoardService::new(project_id);
    service
        .move_task(&task_id, new_parent_id.as_deref(), new_position)
        .map_err(|e| e.to_string())
}

// ========== EVAL COMMANDS ==========

/// Get all evals for a project
#[tauri::command]
pub async fn get_board_evals(project_id: i64) -> Result<Vec<Eval>, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let service = BoardService::new(project_id);
    service.get_evals().map_err(|e| e.to_string())
}

/// Create a new eval
#[tauri::command]
pub async fn create_board_eval(
    project_id: i64,
    name: String,
    content: Option<String>,
    validates: Option<Vec<String>>,
) -> Result<Eval, String> {
    let service = BoardService::new(project_id);
    service
        .create_eval(&CreateEvalRequest {
            name,
            content: content.unwrap_or_default(),
            validates: validates.unwrap_or_default(),
        })
        .map_err(|e| e.to_string())
}

/// Update an eval
#[tauri::command]
pub async fn update_board_eval(
    project_id: i64,
    eval_id: String,
    name: Option<String>,
    status: Option<String>,
    content: Option<String>,
    validates: Option<Vec<String>>,
    x: Option<f64>,
    y: Option<f64>,
) -> Result<Eval, String> {
    let service = BoardService::new(project_id);
    service
        .update_eval(
            &eval_id,
            &UpdateEvalRequest {
                name,
                status: status.map(|s| EvalStatus::from_str(&s)),
                content,
                validates,
                x,
                y,
            },
        )
        .map_err(|e| e.to_string())
}

/// Delete an eval
#[tauri::command]
pub async fn delete_board_eval(project_id: i64, eval_id: String) -> Result<(), String> {
    let service = BoardService::new(project_id);
    service.delete_eval(&eval_id).map_err(|e| e.to_string())
}

// ========== BOOKMARK COMMANDS ==========

/// Get all bookmarks for a project
#[tauri::command]
pub async fn get_bookmarks(project_id: i64) -> Result<Vec<Bookmark>, String> {
    let service = BoardService::new(project_id);
    service.list_bookmarks().map_err(|e| e.to_string())
}

/// Save a bookmark
#[tauri::command]
pub async fn save_bookmark(
    project_id: i64,
    name: String,
    x: f64,
    y: f64,
    zoom: f64,
) -> Result<Bookmark, String> {
    let service = BoardService::new(project_id);
    service
        .save_bookmark(&name, x, y, zoom)
        .map_err(|e| e.to_string())
}

/// Delete a bookmark
#[tauri::command]
pub async fn delete_bookmark(project_id: i64, bookmark_id: String) -> Result<(), String> {
    let service = BoardService::new(project_id);
    service
        .delete_bookmark(&bookmark_id)
        .map_err(|e| e.to_string())
}

// ========== BOARD SYNC COMMANDS ==========

/// Export board to agent JSON files
///
/// Creates/updates per-task JSON files at `~/.hirsel/projects/{project_id}/board/`
/// If focus_task_id/focus_task_name are provided, only exports that task.
/// Returns the path to the board directory.
#[tauri::command]
pub async fn export_board_for_agent(
    project_id: i64,
    focus_task_id: Option<String>,
    focus_task_name: Option<String>,
) -> Result<String, String> {
    let scope = match (focus_task_id, focus_task_name) {
        (Some(task_id), Some(task_name)) => ExportScope::FocusedTask { task_id, task_name },
        _ => ExportScope::WholeBoard,
    };

    let mut service = BoardService::new(project_id);
    let board_dir = service
        .export_for_agent(&scope)
        .await
        .map_err(|e| e.to_string())?;
    Ok(board_dir.to_string_lossy().to_string())
}

/// Import board from agent JSON files
///
/// Reads per-task JSON files from the board directory and syncs to the database.
/// Returns a summary of changes made.
#[tauri::command]
pub async fn import_board_from_agent(project_id: i64) -> Result<SyncResult, String> {
    let mut service = BoardService::new(project_id);
    service.import_from_agent().await.map_err(|e| e.to_string())
}

/// Get the board directory path for a project
#[tauri::command]
pub async fn get_board_directory(project_id: i64) -> Result<String, String> {
    let service = BoardService::new(project_id);
    let board_dir = service.board_dir();
    Ok(board_dir.to_string_lossy().to_string())
}

/// Poll for board file changes and import if changed
///
/// Returns a SyncResult indicating what changed.
#[tauri::command]
pub async fn poll_board_changes(project_id: i64) -> Result<SyncResult, String> {
    let mut service = BoardService::new(project_id);
    service.sync_file_changes().await.map_err(|e| e.to_string())
}
