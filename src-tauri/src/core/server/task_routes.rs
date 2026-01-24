//! Shared task HTTP route handlers.
//!
//! These functions implement the core logic for task API endpoints,
//! used by both daemon (multi-run) and coordinator_api (single-run).
//!
//! Each function takes:
//! - `&SQLiteState` for state access
//!
//! The HTTP layer (daemon/coordinator_api) is responsible for:
//! - Extracting state from headers or shared state
//! - Converting results to HTTP responses

use serde::{Deserialize, Serialize};

use crate::core::state::{SQLiteState, StateResult, Task, TaskStatus};

// =============================================================================
// Response Types
// =============================================================================

#[derive(Serialize)]
pub struct TasksResponse {
    pub tasks: Vec<Task>,
}

#[derive(Serialize)]
pub struct TaskResponse {
    pub task: Option<Task>,
}

#[derive(Serialize)]
pub struct ClaimableTasksResponse {
    pub tasks: Vec<Task>,
}

#[derive(Serialize)]
pub struct BlockedResponse {
    pub blocked: bool,
}

#[derive(Serialize)]
pub struct BlockersResponse {
    pub blockers: Vec<String>,
}

#[derive(Serialize)]
pub struct HasChildrenResponse {
    pub has_children: bool,
}

#[derive(Serialize)]
pub struct ChildrenResponse {
    pub children: Vec<Task>,
}

// =============================================================================
// Request Types
// =============================================================================

#[derive(Deserialize)]
pub struct CreateTaskRequest {
    pub task_id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub blocked_by: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct ClaimTaskRequest {
    pub worker_name: String,
}

#[derive(Deserialize)]
pub struct CompleteTaskRequest {
    pub worker_name: String,
}

#[derive(Deserialize)]
pub struct TokensRequest {
    pub tokens: i64,
}

// =============================================================================
// Handler Functions
// =============================================================================

/// List all tasks
pub fn list_tasks(state: &SQLiteState) -> StateResult<Vec<Task>> {
    state.get_tasks()
}

/// Get claimable tasks (todo status, not blocked)
pub fn get_claimable_tasks(state: &SQLiteState) -> StateResult<Vec<Task>> {
    state.get_claimable_tasks()
}

/// Get a specific task by ID
pub fn get_task(state: &SQLiteState, task_id: &str) -> StateResult<Option<Task>> {
    state.get_task(task_id)
}

/// Create a new task
pub fn create_task(
    state: &SQLiteState,
    task_id: &str,
    name: &str,
    parent_id: Option<&str>,
    blocked_by: Option<&[&str]>,
) -> StateResult<()> {
    state.add_task(task_id, name, parent_id, blocked_by)?;
    Ok(())
}

/// Delete a task
pub fn delete_task(state: &SQLiteState, task_id: &str) -> StateResult<()> {
    state.delete_task(task_id)
}

/// Claim a task for a worker
pub fn claim_task(state: &SQLiteState, task_id: &str, worker_name: &str) -> StateResult<()> {
    state.claim_task(task_id, worker_name)
}

/// Complete a task
pub fn complete_task(state: &SQLiteState, task_id: &str, worker_name: &str) -> StateResult<()> {
    state.complete_task(task_id, worker_name)
}

/// Unclaim a task (release back to pool)
pub fn unclaim_task(state: &SQLiteState, task_id: &str, worker_name: &str) -> StateResult<()> {
    state.unclaim_task(task_id, worker_name)
}

/// Check if a task is blocked
pub fn is_task_blocked(state: &SQLiteState, task_id: &str) -> StateResult<bool> {
    state.is_task_blocked(task_id)
}

/// Get blockers for a task
pub fn get_blockers(state: &SQLiteState, task_id: &str) -> StateResult<Vec<String>> {
    state.get_blockers(task_id)
}

/// Check if task has children
pub fn has_children(state: &SQLiteState, task_id: &str) -> StateResult<bool> {
    state.has_children(task_id)
}

/// Get children of a task
pub fn get_children(state: &SQLiteState, task_id: &str) -> StateResult<Vec<Task>> {
    state.get_children(task_id)
}

/// Set task pending done timestamp
pub fn set_pending_done(state: &SQLiteState, task_id: &str) -> StateResult<()> {
    state.set_task_pending_done(task_id)
}

/// Clear task pending done timestamp
pub fn clear_pending_done(state: &SQLiteState, task_id: &str) -> StateResult<()> {
    state.clear_task_pending_done(task_id)
}

/// Reopen a completed task
pub fn reopen_task(state: &SQLiteState, task_id: &str) -> StateResult<()> {
    state.reopen_task(task_id)
}

/// Set tokens used for a task
pub fn set_task_tokens(state: &SQLiteState, task_id: &str, tokens: i64) -> StateResult<()> {
    state.set_task_tokens(task_id, tokens)
}

/// Mark a task as done (admin operation, no worker required)
pub fn mark_task_done(state: &SQLiteState, task_id: &str) -> StateResult<()> {
    // Get the task to check status
    let task = state
        .get_task(task_id)?
        .ok_or_else(|| crate::core::state::StateError::NotFound(format!("Task '{}'", task_id)))?;

    match task.status {
        TaskStatus::Done => Ok(()), // Already done
        TaskStatus::Doing => {
            // If being worked on, complete it with a placeholder worker name
            state.complete_task(task_id, "admin")
        }
        TaskStatus::Todo => {
            // Claim and complete in one go
            state.claim_task(task_id, "admin")?;
            state.complete_task(task_id, "admin")
        }
    }
}

/// Get count of done vs total tasks
pub fn get_task_counts(state: &SQLiteState) -> StateResult<(usize, usize)> {
    let tasks = state.get_tasks()?;
    let done = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Done)
        .count();
    let total = tasks.len();
    Ok((done, total))
}
