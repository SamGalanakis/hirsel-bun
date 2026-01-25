//! SpecFlow board commands
//!
//! Commands for managing the SpecFlow canvas: islands, rows, wires, and bookmarks.

use crate::core::specflow::{
    Bookmark, CreateIslandRequest, CreateRowRequest, Island, Row, RowEvalStatus, SpecFlowState,
    SpecStatus, TaskStatus, UpdateIslandRequest, UpdateRowRequest, Wire,
};
use crate::core::ProjectStore;

/// Get all islands for a project (with rows)
#[tauri::command]
pub async fn get_project_islands(project_id: i64) -> Result<Vec<Island>, String> {
    // Verify project exists
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state.list_islands().map_err(|e| e.to_string())
}

/// Create a new island
#[tauri::command]
pub async fn create_island(
    project_id: i64,
    name: String,
    x: f64,
    y: f64,
    width: Option<f64>,
) -> Result<Island, String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state
        .create_island(&CreateIslandRequest {
            name,
            x,
            y,
            width: width.unwrap_or(400.0),
        })
        .map_err(|e| e.to_string())
}

/// Update an island
#[tauri::command]
pub async fn update_island(
    project_id: i64,
    island_id: String,
    name: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    collapsed: Option<bool>,
    summary: Option<String>,
) -> Result<Island, String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state
        .update_island(
            &island_id,
            &UpdateIslandRequest {
                name,
                x,
                y,
                width,
                collapsed,
                summary,
            },
        )
        .map_err(|e| e.to_string())
}

/// Delete an island
#[tauri::command]
pub async fn delete_island(project_id: i64, island_id: String) -> Result<(), String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state.delete_island(&island_id).map_err(|e| e.to_string())
}

/// Create a new row in an island
#[tauri::command]
pub async fn create_row(
    project_id: i64,
    island_id: String,
    position: Option<i32>,
) -> Result<Row, String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state
        .create_row(&CreateRowRequest {
            island_id,
            position,
        })
        .map_err(|e| e.to_string())
}

/// Update a row
#[tauri::command]
pub async fn update_row(
    project_id: i64,
    row_id: String,
    spec_content: Option<String>,
    spec_status: Option<String>,
    task_title: Option<String>,
    task_description: Option<String>,
    task_status: Option<String>,
    task_worker: Option<String>,
    task_blocked_by: Option<Vec<String>>,
    eval_criterion: Option<String>,
    eval_status: Option<String>,
    eval_result: Option<String>,
) -> Result<Row, String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;

    let req = UpdateRowRequest {
        spec_content,
        spec_status: spec_status.map(|s| SpecStatus::from_str(&s)),
        task_title,
        task_description,
        task_status: task_status.map(|s| TaskStatus::from_str(&s)),
        task_worker,
        task_blocked_by,
        eval_criterion,
        eval_status: eval_status.map(|s| RowEvalStatus::from_str(&s)),
        eval_result,
    };

    state.update_row(&row_id, &req).map_err(|e| e.to_string())
}

/// Delete a row
#[tauri::command]
pub async fn delete_row(project_id: i64, row_id: String) -> Result<(), String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state.delete_row(&row_id).map_err(|e| e.to_string())
}

/// Reorder rows within an island
#[tauri::command]
pub async fn reorder_rows(
    project_id: i64,
    island_id: String,
    row_ids: Vec<String>,
) -> Result<(), String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state
        .reorder_rows(&island_id, &row_ids)
        .map_err(|e| e.to_string())
}

/// Get all wires for a project
#[tauri::command]
pub async fn get_wires(project_id: i64) -> Result<Vec<Wire>, String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state.list_wires().map_err(|e| e.to_string())
}

/// Create a wire between islands
#[tauri::command]
pub async fn create_wire(
    project_id: i64,
    from_island_id: String,
    to_island_id: String,
) -> Result<Wire, String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state
        .create_wire(&from_island_id, &to_island_id)
        .map_err(|e| e.to_string())
}

/// Delete a wire
#[tauri::command]
pub async fn delete_wire(project_id: i64, wire_id: String) -> Result<(), String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state.delete_wire(&wire_id).map_err(|e| e.to_string())
}

/// Get all bookmarks for a project
#[tauri::command]
pub async fn get_bookmarks(project_id: i64) -> Result<Vec<Bookmark>, String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state.list_bookmarks().map_err(|e| e.to_string())
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
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state
        .save_bookmark(&name, x, y, zoom)
        .map_err(|e| e.to_string())
}

/// Delete a bookmark
#[tauri::command]
pub async fn delete_bookmark(project_id: i64, bookmark_id: String) -> Result<(), String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state
        .delete_bookmark(&bookmark_id)
        .map_err(|e| e.to_string())
}

/// Dispatch selected rows to create a run
/// Returns Warning if any rows are already dispatched, Success with run name otherwise
#[tauri::command]
pub async fn dispatch_rows(
    project_id: i64,
    row_ids: Vec<String>,
) -> Result<crate::core::specflow::DispatchResult, String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;

    // Check if any rows are already dispatched
    let already_dispatched = state
        .check_already_dispatched(&row_ids)
        .map_err(|e| e.to_string())?;

    if !already_dispatched.is_empty() {
        return Ok(crate::core::specflow::DispatchResult::Warning {
            message: format!(
                "{} row(s) are already in active runs",
                already_dispatched.len()
            ),
            row_ids: already_dispatched,
        });
    }

    // Get rows with dependencies
    let rows = state
        .get_rows_with_deps(&row_ids)
        .map_err(|e| e.to_string())?;

    // For now, just return success - actual run creation will be added later
    // when we integrate with the draft/run system
    let run_name = format!("specflow-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));

    // Mark rows as dispatched
    for row in &rows {
        state
            .set_row_dispatched(&row.id, &run_name)
            .map_err(|e| e.to_string())?;
    }

    Ok(crate::core::specflow::DispatchResult::Success { run_name })
}

/// Dispatch rows after warning was acknowledged (force dispatch)
#[tauri::command]
pub async fn dispatch_rows_confirm(
    project_id: i64,
    row_ids: Vec<String>,
) -> Result<String, String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;

    // Get rows with dependencies (even already dispatched ones)
    let rows = state
        .get_rows_with_deps(&row_ids)
        .map_err(|e| e.to_string())?;

    let run_name = format!("specflow-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));

    // Mark rows as dispatched
    for row in &rows {
        state
            .set_row_dispatched(&row.id, &run_name)
            .map_err(|e| e.to_string())?;
    }

    Ok(run_name)
}

/// Sync run status back to board rows
#[tauri::command]
pub async fn sync_run_status(project_id: i64, run_name: String) -> Result<(), String> {
    use crate::core::{config, state::SQLiteState};

    let specflow_state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;

    // Open the run's database
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let run_state =
        SQLiteState::new(db_path).map_err(|e| format!("Failed to open run database: {}", e))?;

    // Get all tasks from the run
    let tasks = run_state
        .get_tasks()
        .map_err(|e| format!("Failed to get tasks: {}", e))?;

    // Update board rows based on task status
    for task in tasks {
        // Check if this task has a source_row_id in its metadata
        // For now, we'll use the task ID as the row ID if it matches UUID format
        // This will be improved when we properly link tasks to rows

        let row_status = match task.status {
            crate::core::state::TaskStatus::Todo => TaskStatus::Todo,
            crate::core::state::TaskStatus::Doing => TaskStatus::Doing,
            crate::core::state::TaskStatus::Done => TaskStatus::Done,
        };

        // Try to update the row (ignore errors if row doesn't exist)
        let _ = specflow_state.update_row_task_status(&task.id, row_status, task.claimed_by);
    }

    Ok(())
}

/// Set task dependencies (blocked_by) for a row
#[tauri::command]
pub async fn set_task_blocked_by(
    project_id: i64,
    row_id: String,
    blocked_by_ids: Vec<String>,
) -> Result<Row, String> {
    let state = SpecFlowState::open(project_id).map_err(|e| e.to_string())?;
    state
        .update_row(
            &row_id,
            &UpdateRowRequest {
                task_blocked_by: Some(blocked_by_ids),
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())
}

// ========== BOARD SYNC COMMANDS ==========

/// Export board to agent JSON files
///
/// Creates/updates JSON files at `~/.hirsel/projects/{project_id}/board/{island-id}.json`
/// Returns the path to the board directory.
#[tauri::command]
pub async fn export_board_for_agent(project_id: i64) -> Result<String, String> {
    use crate::core::BoardService;

    let service = BoardService::new(project_id);
    let board_dir = service
        .export_for_agent()
        .await
        .map_err(|e| e.to_string())?;
    Ok(board_dir.to_string_lossy().to_string())
}

/// Import board from agent JSON files
///
/// Reads JSON files from the board directory and syncs them to the database.
/// Returns a summary of changes made.
#[tauri::command]
pub async fn import_board_from_agent(
    project_id: i64,
) -> Result<crate::core::BoardSyncResult, String> {
    use crate::core::BoardService;

    let service = BoardService::new(project_id);
    service.import_from_agent().await.map_err(|e| e.to_string())
}

/// Get the board directory path for a project
#[tauri::command]
pub async fn get_board_directory(project_id: i64) -> Result<String, String> {
    use crate::core::BoardService;

    let service = BoardService::new(project_id);
    let board_dir = service.board_dir();
    Ok(board_dir.to_string_lossy().to_string())
}
