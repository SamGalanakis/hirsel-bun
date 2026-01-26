//! Task-related commands
//!
//! Commands for managing tasks: listing, adding, deleting, completing, unclaiming, and reopening.

use super::types::{Task, TaskStatus};
use crate::core::orchestrator::create_orchestrator;
use crate::core::{config, state::SQLiteState};

/// Get all tasks for a run
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn get_tasks(run_name: String) -> Result<Vec<Task>, String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.list_tasks(&run_name).await.map_err(|e| e.to_string())
}

/// Add a new task
#[tauri::command]
pub async fn add_task(
    run_name: String,
    task_id: String,
    description: String,
    parent_id: Option<String>,
    blocked_by: Option<Vec<String>>,
) -> Result<Task, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Convert blocked_by from Vec<String> to Vec<&str> for state.add_task
    let blocked_by_refs: Option<Vec<&str>> = blocked_by
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect());
    let blocked_by_slice: Option<&[&str]> = blocked_by_refs.as_deref();

    // Add the task
    state
        .add_task(
            &task_id,
            &description,
            parent_id.as_deref(),
            blocked_by_slice,
        )
        .map_err(|e| format!("Failed to add task: {}", e))?;

    // Return the created task
    let task = state
        .get_task(&task_id)
        .map_err(|e| format!("Failed to get task: {}", e))?
        .ok_or_else(|| "Task not found after creation".to_string())?;

    // Convert blocked_by from comma-separated string to Vec
    let blocked_by_vec = task
        .blocked_by
        .as_ref()
        .map(|s| {
            s.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|v: &Vec<String>| !v.is_empty());

    Ok(Task {
        id: task.id,
        description: task.name,
        status: match task.status {
            crate::core::state::TaskStatus::Todo => TaskStatus::Todo,
            crate::core::state::TaskStatus::Doing => TaskStatus::Doing,
            crate::core::state::TaskStatus::Done => TaskStatus::Done,
            crate::core::state::TaskStatus::AwaitingEval => TaskStatus::AwaitingEval,
            crate::core::state::TaskStatus::Validated => TaskStatus::Validated,
            crate::core::state::TaskStatus::NeedsRepair => TaskStatus::NeedsRepair,
        },
        claimed_by: task.claimed_by,
        claimed_at: task.claimed_at,
        completed_at: task.completed_at,
        parent_id: task.parent_id,
        blocked_by: blocked_by_vec,
        tokens_used: task.tokens_used.map(|t| t as u64),
        created_at: task.created_at,
    })
}

/// Delete a task
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn delete_task(run_name: String, task_id: String) -> Result<(), String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.delete_task(&run_name, &task_id)
        .await
        .map_err(|e| e.to_string())
}

/// Mark a task as complete (from UI - uses "user" as worker name)
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn complete_task(run_name: String, task_id: String) -> Result<(), String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.complete_task(&run_name, &task_id)
        .await
        .map_err(|e| e.to_string())
}

/// Unclaim a task (release it back to the pool)
#[tauri::command]
pub async fn unclaim_task(run_name: String, task_id: String) -> Result<(), String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Get the task to find who claimed it
    let task = state
        .get_task(&task_id)
        .map_err(|e| format!("Failed to get task: {}", e))?
        .ok_or_else(|| format!("Task '{}' not found", task_id))?;

    let worker = task.claimed_by.unwrap_or_else(|| "user".to_string());

    state
        .unclaim_task(&task_id, &worker)
        .map_err(|e| format!("Failed to unclaim task: {}", e))?;

    Ok(())
}

/// Reopen a completed task
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn reopen_task(run_name: String, task_id: String) -> Result<(), String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.reopen_task(&run_name, &task_id)
        .await
        .map_err(|e| e.to_string())
}
