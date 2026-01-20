//! Task management CLI commands.
//!
//! Provides the implementation for task-related CLI commands:
//! - `hirsel tasks <run>` - List tasks in a run
//! - `hirsel task-add <run> <id> <description>` - Add a task
//! - `hirsel task-delete <run> <id>` - Delete a task
//! - `hirsel task-done <run> <id>` - Mark task as done (admin)
//! - `hirsel task-reopen <run> <id>` - Reopen a completed task
//! - `hirsel task-unclaim <run> <id>` - Unclaim a task

use crate::cli::config::hirsel_root;
use crate::core::state::{SQLiteState, StateError, Task, TaskStatus};
use serde::Serialize;
use std::path::PathBuf;

/// Errors that can occur during task operations.
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("Run '{0}' not found")]
    RunNotFound(String),

    #[error("Task '{0}' not found")]
    TaskNotFound(String),

    #[error("State error: {0}")]
    State(#[from] StateError),

    #[error("Failed to serialize: {0}")]
    Serialization(String),
}

/// Task display information for output.
#[derive(Debug, Clone, Serialize)]
pub struct TaskDisplay {
    pub id: String,
    pub name: String,
    pub status: String,
    pub claimed_by: Option<String>,
    pub parent_id: Option<String>,
    pub blocked_by: Option<String>,
    pub depth: usize,
}

impl TaskDisplay {
    fn from_task(task: &Task, depth: usize) -> Self {
        Self {
            id: task.id.clone(),
            name: task.name.clone(),
            status: task.status.to_string(),
            claimed_by: task.claimed_by.clone(),
            parent_id: task.parent_id.clone(),
            blocked_by: task.blocked_by.clone(),
            depth,
        }
    }
}

/// Get the run directory path.
fn get_run_dir(run_name: &str) -> Result<PathBuf, TaskError> {
    let run_dir = hirsel_root().join("runs").join(run_name);
    if !run_dir.exists() {
        return Err(TaskError::RunNotFound(run_name.to_string()));
    }
    Ok(run_dir)
}

/// Get the state for a run.
fn get_state(run_name: &str) -> Result<SQLiteState, TaskError> {
    let run_dir = get_run_dir(run_name)?;
    let db_path = run_dir.join("hirsel.db");
    SQLiteState::new(db_path).map_err(TaskError::State)
}

/// Execute the `hirsel tasks` command.
///
/// Lists all tasks in a run with their status, hierarchy, and assignments.
pub fn run_tasks(run_name: &str, json_output: bool) -> Result<String, TaskError> {
    let state = get_state(run_name)?;
    let tasks = state.get_tasks().map_err(TaskError::State)?;

    if json_output {
        let displays: Vec<TaskDisplay> = tasks
            .iter()
            .map(|t| {
                let depth = state.get_task_depth(&t.id).unwrap_or(0);
                TaskDisplay::from_task(t, depth)
            })
            .collect();

        return serde_json::to_string_pretty(&serde_json::json!({
            "run": run_name,
            "tasks": displays
        }))
        .map_err(|e| TaskError::Serialization(e.to_string()));
    }

    if tasks.is_empty() {
        return Ok(format!("No tasks in run '{}'\n", run_name));
    }

    let mut output = format!("Tasks in '{}'\n\n", run_name);

    // Build task hierarchy for display
    fn format_task_tree(
        state: &SQLiteState,
        tasks: &[Task],
        parent_id: Option<&str>,
        depth: usize,
        output: &mut String,
    ) {
        let children: Vec<&Task> = tasks
            .iter()
            .filter(|t| t.parent_id.as_deref() == parent_id)
            .collect();

        for task in children {
            let indent = "  ".repeat(depth);
            let status_icon = match task.status {
                TaskStatus::Todo => "○",
                TaskStatus::Doing => "◐",
                TaskStatus::Done => "●",
            };

            let claimed = task
                .claimed_by
                .as_ref()
                .map(|c| format!(" ({})", c))
                .unwrap_or_default();

            let blocked = if let Some(blockers) = &task.blocked_by {
                if !blockers.is_empty() {
                    // Check if actually blocked
                    if let Ok(true) = state.is_task_blocked(&task.id) {
                        " [blocked]".to_string()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            output.push_str(&format!(
                "{}{} {} - {}{}{}\n",
                indent, status_icon, task.id, task.name, claimed, blocked
            ));

            // Recurse for children
            format_task_tree(state, tasks, Some(&task.id), depth + 1, output);
        }
    }

    format_task_tree(&state, &tasks, None, 0, &mut output);

    // Summary
    let todo_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Todo)
        .count();
    let doing_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Doing)
        .count();
    let done_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Done)
        .count();

    output.push_str(&format!(
        "\n{} todo, {} in progress, {} done\n",
        todo_count, doing_count, done_count
    ));

    Ok(output)
}

/// Execute the `hirsel task-add` command.
///
/// Adds a new task to a run.
pub fn run_task_add(
    run_name: &str,
    task_id: &str,
    description: &str,
    parent: Option<&str>,
    blocked_by: &[String],
    json_output: bool,
) -> Result<String, TaskError> {
    let state = get_state(run_name)?;

    // Convert Vec<String> to Vec<&str> for the API
    let blocked_by_refs: Vec<&str> = blocked_by.iter().map(|s| s.as_str()).collect();

    state
        .add_task(
            task_id,
            description,
            parent,
            if blocked_by_refs.is_empty() {
                None
            } else {
                Some(blocked_by_refs.as_slice())
            },
        )
        .map_err(TaskError::State)?;

    if json_output {
        return serde_json::to_string_pretty(&serde_json::json!({
            "success": true,
            "task_id": task_id,
            "message": format!("Task '{}' added", task_id)
        }))
        .map_err(|e| TaskError::Serialization(e.to_string()));
    }

    Ok(format!("Added task '{}': {}\n", task_id, description))
}

/// Execute the `hirsel task-delete` command.
///
/// Deletes a task and all its children from a run.
pub fn run_task_delete(
    run_name: &str,
    task_id: &str,
    json_output: bool,
) -> Result<String, TaskError> {
    let state = get_state(run_name)?;

    state.delete_task(task_id).map_err(TaskError::State)?;

    if json_output {
        return serde_json::to_string_pretty(&serde_json::json!({
            "success": true,
            "task_id": task_id,
            "message": format!("Task '{}' deleted", task_id)
        }))
        .map_err(|e| TaskError::Serialization(e.to_string()));
    }

    Ok(format!("Deleted task '{}'\n", task_id))
}

/// Execute the `hirsel task-done` command.
///
/// Marks a task as done (admin override, doesn't require claim).
pub fn run_task_done(
    run_name: &str,
    task_id: &str,
    json_output: bool,
) -> Result<String, TaskError> {
    let state = get_state(run_name)?;

    // Get the task first
    let task = state
        .get_task(task_id)
        .map_err(TaskError::State)?
        .ok_or_else(|| TaskError::TaskNotFound(task_id.to_string()))?;

    // Admin override: directly set status to done
    // This bypasses the normal claim check
    state
        .admin_complete_task(task_id)
        .map_err(TaskError::State)?;

    if json_output {
        return serde_json::to_string_pretty(&serde_json::json!({
            "success": true,
            "task_id": task_id,
            "message": format!("Task '{}' marked as done", task_id)
        }))
        .map_err(|e| TaskError::Serialization(e.to_string()));
    }

    let was_claimed = task.claimed_by.is_some();
    if was_claimed {
        Ok(format!(
            "Task '{}' marked as done (was claimed by {})\n",
            task_id,
            task.claimed_by.unwrap_or_default()
        ))
    } else {
        Ok(format!("Task '{}' marked as done\n", task_id))
    }
}

/// Execute the `hirsel task-reopen` command.
///
/// Reopens a completed task.
pub fn run_task_reopen(
    run_name: &str,
    task_id: &str,
    json_output: bool,
) -> Result<String, TaskError> {
    let state = get_state(run_name)?;

    state.reopen_task(task_id).map_err(TaskError::State)?;

    if json_output {
        return serde_json::to_string_pretty(&serde_json::json!({
            "success": true,
            "task_id": task_id,
            "message": format!("Task '{}' reopened", task_id)
        }))
        .map_err(|e| TaskError::Serialization(e.to_string()));
    }

    Ok(format!("Task '{}' reopened\n", task_id))
}

/// Execute the `hirsel task-unclaim` command.
///
/// Unclaims a task (admin override, releases any worker's claim).
pub fn run_task_unclaim(
    run_name: &str,
    task_id: &str,
    json_output: bool,
) -> Result<String, TaskError> {
    let state = get_state(run_name)?;

    // Get the task first
    let task = state
        .get_task(task_id)
        .map_err(TaskError::State)?
        .ok_or_else(|| TaskError::TaskNotFound(task_id.to_string()))?;

    let claimed_by = task.claimed_by.clone();

    // Admin override: directly unclaim
    state
        .admin_unclaim_task(task_id)
        .map_err(TaskError::State)?;

    if json_output {
        return serde_json::to_string_pretty(&serde_json::json!({
            "success": true,
            "task_id": task_id,
            "previous_owner": claimed_by,
            "message": format!("Task '{}' unclaimed", task_id)
        }))
        .map_err(|e| TaskError::Serialization(e.to_string()));
    }

    match claimed_by {
        Some(owner) => Ok(format!(
            "Task '{}' unclaimed (was held by {})\n",
            task_id, owner
        )),
        None => Ok(format!("Task '{}' was not claimed\n", task_id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_display_from_task() {
        let task = Task {
            id: "test".to_string(),
            name: "Test task".to_string(),
            status: TaskStatus::Todo,
            created_at: "2024-01-01".to_string(),
            completed_at: None,
            claimed_by: Some("worker1".to_string()),
            claimed_at: None,
            pending_done_at: None,
            tokens_used: None,
            parent_id: None,
            blocked_by: Some("other".to_string()),
        };

        let display = TaskDisplay::from_task(&task, 2);
        assert_eq!(display.id, "test");
        assert_eq!(display.status, "todo");
        assert_eq!(display.claimed_by, Some("worker1".to_string()));
        assert_eq!(display.depth, 2);
    }

    #[test]
    fn test_get_run_dir_not_found() {
        let result = get_run_dir("nonexistent-run");
        assert!(matches!(result, Err(TaskError::RunNotFound(_))));
    }
}
