//! Task management CLI commands.
//!
//! Provides the implementation for task-related CLI commands:
//! - `hirsel tasks <run>` - List tasks in a run
//! - `hirsel task-add <run> <id> <description>` - Add a task
//! - `hirsel task-delete <run> <id>` - Delete a task
//! - `hirsel task-done <run> <id>` - Mark task as done (admin)
//! - `hirsel task-reopen <run> <id>` - Reopen a completed task
//! - `hirsel task-unclaim <run> <id>` - Unclaim a task
//!
//! All commands use live_nodes from the DeltaState (global database).

use crate::cli::config::hirsel_root;
use crate::core::delta::{DeltaState, DeltaStateError, LiveNode, LiveNodeStatus, NodeType};
use crate::core::state::{SQLiteState, StateError};
use serde::Serialize;
use std::path::PathBuf;

/// Errors that can occur during task operations.
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("Run '{0}' not found")]
    RunNotFound(String),

    #[error("Task '{0}' not found")]
    TaskNotFound(String),

    #[error("Run is not linked to a project")]
    NotProjectRun,

    #[error("State error: {0}")]
    State(#[from] StateError),

    #[error("Delta state error: {0}")]
    DeltaState(#[from] DeltaStateError),

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
    pub blocked_by: Vec<String>,
    pub depth: usize,
}

impl TaskDisplay {
    fn from_node(node: &LiveNode, depth: usize) -> Self {
        Self {
            id: node.id.clone(),
            name: node.name.clone(),
            status: node.status.as_str().to_string(),
            claimed_by: node.claimed_by.clone(),
            parent_id: node.parent_id.clone(),
            blocked_by: node.blocked_by.clone(),
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

/// Get the SQLiteState and project_id for a run.
fn get_project_state(run_name: &str) -> Result<(SQLiteState, i64), TaskError> {
    let run_dir = get_run_dir(run_name)?;
    let db_path = run_dir.join("hirsel.db");
    let state = SQLiteState::new(db_path).map_err(TaskError::State)?;

    let project_id = state
        .get_project_id()
        .map_err(TaskError::State)?
        .ok_or(TaskError::NotProjectRun)?;

    Ok((state, project_id))
}

/// Calculate depth of a node in the tree.
fn get_node_depth(nodes: &[LiveNode], node_id: &str) -> usize {
    let node = nodes.iter().find(|n| n.id == node_id);
    match node {
        Some(n) => match &n.parent_id {
            Some(pid) => 1 + get_node_depth(nodes, pid),
            None => 0,
        },
        None => 0,
    }
}

/// Execute the `hirsel tasks` command.
///
/// Lists all tasks in a run with their status, hierarchy, and assignments.
pub fn run_tasks(run_name: &str, json_output: bool) -> Result<String, TaskError> {
    let (_, project_id) = get_project_state(run_name)?;
    let delta_state = DeltaState::new(project_id);
    let nodes = delta_state.get_live_nodes()?;

    if json_output {
        let displays: Vec<TaskDisplay> = nodes
            .iter()
            .map(|n| {
                let depth = get_node_depth(&nodes, &n.id);
                TaskDisplay::from_node(n, depth)
            })
            .collect();

        return serde_json::to_string_pretty(&serde_json::json!({
            "run": run_name,
            "tasks": displays
        }))
        .map_err(|e| TaskError::Serialization(e.to_string()));
    }

    if nodes.is_empty() {
        return Ok(format!("No tasks in run '{}'\n", run_name));
    }

    let mut output = format!("Tasks in '{}'\n\n", run_name);

    // Build task hierarchy for display
    fn format_node_tree(
        delta_state: &DeltaState,
        nodes: &[LiveNode],
        parent_id: Option<&str>,
        depth: usize,
        output: &mut String,
    ) {
        let children: Vec<&LiveNode> = nodes
            .iter()
            .filter(|n| n.parent_id.as_deref() == parent_id)
            .collect();

        for node in children {
            let indent = "  ".repeat(depth);
            let status_icon = match node.status {
                LiveNodeStatus::Pending => "○",
                LiveNodeStatus::Working => "◐",
                LiveNodeStatus::Done => "●",
                LiveNodeStatus::AwaitingEval => "◔",
                LiveNodeStatus::Validated => "✔",
                LiveNodeStatus::NeedsRepair => "⚒",
                LiveNodeStatus::Failed => "✗",
            };

            let claimed = node
                .claimed_by
                .as_ref()
                .map(|c| format!(" ({})", c))
                .unwrap_or_default();

            let blocked = if !node.blocked_by.is_empty() {
                // Check if actually blocked
                if let Ok(true) = delta_state.is_node_blocked(&node.id) {
                    " [blocked]".to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            output.push_str(&format!(
                "{}{} {} - {}{}{}\n",
                indent, status_icon, node.id, node.name, claimed, blocked
            ));

            // Recurse for children
            format_node_tree(delta_state, nodes, Some(&node.id), depth + 1, output);
        }
    }

    format_node_tree(&delta_state, &nodes, None, 0, &mut output);

    // Summary
    let pending_count = nodes
        .iter()
        .filter(|n| n.status == LiveNodeStatus::Pending)
        .count();
    let working_count = nodes
        .iter()
        .filter(|n| n.status == LiveNodeStatus::Working)
        .count();
    let done_count = nodes
        .iter()
        .filter(|n| matches!(n.status, LiveNodeStatus::Done | LiveNodeStatus::Validated))
        .count();

    output.push_str(&format!(
        "\n{} pending, {} in progress, {} done\n",
        pending_count, working_count, done_count
    ));

    Ok(output)
}

/// Execute the `hirsel task-add` command.
///
/// Adds a new task to a run as a live_node.
pub fn run_task_add(
    run_name: &str,
    task_id: &str,
    description: &str,
    parent: Option<&str>,
    blocked_by: &[String],
    json_output: bool,
) -> Result<String, TaskError> {
    let (_, project_id) = get_project_state(run_name)?;
    let delta_state = DeltaState::new(project_id);

    // Convert Vec<String> to Vec<&str> for the API
    let blocked_by_refs: Vec<&str> = blocked_by.iter().map(|s| s.as_str()).collect();

    delta_state.create_live_node_from_worker(
        task_id,
        description,
        parent,
        if blocked_by_refs.is_empty() {
            None
        } else {
            Some(blocked_by_refs.as_slice())
        },
        NodeType::Task,
        "", // content - empty for CLI-added tasks
    )?;

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
    let (_, project_id) = get_project_state(run_name)?;
    let delta_state = DeltaState::new(project_id);

    delta_state.delete_live_node(task_id)?;

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
    let (_, project_id) = get_project_state(run_name)?;
    let delta_state = DeltaState::new(project_id);

    // Get the node first
    let node = match delta_state.get_live_node(task_id) {
        Ok(n) => n,
        Err(DeltaStateError::LiveNodeNotFound(_)) => {
            return Err(TaskError::TaskNotFound(task_id.to_string()))
        }
        Err(e) => return Err(TaskError::DeltaState(e)),
    };

    let claimed_by = node.claimed_by.clone();

    // Admin override: directly update status to done
    delta_state.update_live_node_status(task_id, LiveNodeStatus::Done, None)?;

    if json_output {
        return serde_json::to_string_pretty(&serde_json::json!({
            "success": true,
            "task_id": task_id,
            "message": format!("Task '{}' marked as done", task_id)
        }))
        .map_err(|e| TaskError::Serialization(e.to_string()));
    }

    match claimed_by {
        Some(owner) => Ok(format!(
            "Task '{}' marked as done (was claimed by {})\n",
            task_id, owner
        )),
        None => Ok(format!("Task '{}' marked as done\n", task_id)),
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
    let (_, project_id) = get_project_state(run_name)?;
    let delta_state = DeltaState::new(project_id);

    // Verify task exists
    match delta_state.get_live_node(task_id) {
        Ok(_) => {}
        Err(DeltaStateError::LiveNodeNotFound(_)) => {
            return Err(TaskError::TaskNotFound(task_id.to_string()))
        }
        Err(e) => return Err(TaskError::DeltaState(e)),
    };

    // Reopen by setting status back to Pending
    delta_state.update_live_node_status(task_id, LiveNodeStatus::Pending, None)?;

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
    let (_, project_id) = get_project_state(run_name)?;
    let delta_state = DeltaState::new(project_id);

    // Get the node first
    let node = match delta_state.get_live_node(task_id) {
        Ok(n) => n,
        Err(DeltaStateError::LiveNodeNotFound(_)) => {
            return Err(TaskError::TaskNotFound(task_id.to_string()))
        }
        Err(e) => return Err(TaskError::DeltaState(e)),
    };

    let claimed_by = node.claimed_by.clone();

    // Unclaim the node
    delta_state.unclaim_live_node(task_id)?;

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
    fn test_get_run_dir_not_found() {
        let result = get_run_dir("nonexistent-run");
        assert!(matches!(result, Err(TaskError::RunNotFound(_))));
    }
}
