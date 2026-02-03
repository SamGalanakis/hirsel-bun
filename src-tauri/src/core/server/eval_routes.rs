//! Shared eval HTTP route handlers.
//!
//! These functions implement the core logic for eval API endpoints,
//! used by both daemon (multi-run) and coordinator_api (single-run).
//!
//! Each function takes:
//! - `&SQLiteState` for state access
//!
//! The HTTP layer (daemon/coordinator_api) is responsible for:
//! - Extracting state from headers or shared state
//! - Converting results to HTTP responses

use serde::{Deserialize, Serialize};

use crate::core::state::{Eval, SQLiteState, StateResult};

// =============================================================================
// Response Types
// =============================================================================

#[derive(Serialize)]
pub struct EvalsResponse {
    pub evals: Vec<Eval>,
}

#[derive(Serialize)]
pub struct EvalResponse {
    pub eval: Option<Eval>,
}

#[derive(Serialize)]
pub struct EvalIdResponse {
    pub id: i64,
}

#[derive(Serialize)]
pub struct CancelEvalsResponse {
    pub count: i64,
}

// =============================================================================
// Request Types
// =============================================================================

#[derive(Deserialize)]
pub struct CreateEvalRequest {
    pub branch: String,
    pub eval_name: Option<String>,
    pub log_file: Option<String>,
}

#[derive(Deserialize)]
pub struct CompleteEvalRequest {
    pub success: bool,
    pub feedback: String,
}

#[derive(Deserialize)]
pub struct ReasonRequest {
    pub reason: String,
}

// =============================================================================
// Handler Functions
// =============================================================================

/// List all evals
pub async fn list_evals(state: &SQLiteState, limit: i64) -> StateResult<Vec<Eval>> {
    state.get_evals(limit).await
}

/// Get a specific eval by ID
pub async fn get_eval(state: &SQLiteState, eval_id: i64) -> StateResult<Option<Eval>> {
    state.get_eval(eval_id).await
}

/// Get eval by name
pub async fn get_eval_by_name(state: &SQLiteState, eval_name: &str) -> StateResult<Option<Eval>> {
    state.get_eval_by_name(eval_name).await
}

/// Get currently running eval
pub async fn get_running_eval(state: &SQLiteState) -> StateResult<Option<Eval>> {
    state.get_running_eval().await
}

/// Start a new eval
pub async fn start_eval(
    state: &SQLiteState,
    branch: &str,
    eval_name: Option<&str>,
    log_file: Option<&str>,
) -> StateResult<i64> {
    state.start_eval(branch, eval_name, log_file).await
}

/// Complete an eval with success/failure status
pub async fn complete_eval(
    state: &SQLiteState,
    eval_id: i64,
    success: bool,
    feedback: &str,
) -> StateResult<()> {
    state.complete_eval(eval_id, success, feedback).await
}

/// Cancel all running evals with a reason
pub async fn cancel_running_evals(state: &SQLiteState, reason: &str) -> StateResult<i64> {
    state.cancel_running_evals(reason).await
}

/// Set the PID of a running eval
pub async fn set_eval_pid(state: &SQLiteState, eval_id: i64, pid: u32) -> StateResult<()> {
    state.set_eval_pid(eval_id, pid).await
}

/// Check if there's a paused eval
pub async fn has_paused_eval(state: &SQLiteState) -> StateResult<bool> {
    state.has_paused_eval().await
}

/// Clear paused eval markers (after resume)
pub async fn clear_paused_evals(state: &SQLiteState) -> StateResult<()> {
    state.clear_paused_evals().await
}
