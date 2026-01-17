//! API route handlers

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::AppState;
use crate::core::api_types::{
    ConfigResponse, Eval, HistoryEntry, Message, RunDetail, RunSummary, Task, ThreadSummary,
    Worker, WorkerEventsResponse,
};
use crate::core::orchestrator::{
    AddTaskRequest, DeliverRunRequest, HealthResponse, Orchestrator, OrchestratorError,
    ResumeRunRequest, SendMessageRequest, WorkerLogParams,
};

/// Convert OrchestratorError to HTTP response
impl IntoResponse for OrchestratorError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            OrchestratorError::RunNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            OrchestratorError::WorkerNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            OrchestratorError::TaskNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            OrchestratorError::InvalidOperation(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            OrchestratorError::UnknownProfile(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        (status, Json(ErrorResponse { error: message })).into_response()
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

type Result<T> = std::result::Result<T, OrchestratorError>;

// =============================================================================
// Health
// =============================================================================

pub async fn health(State(state): State<Arc<AppState>>) -> Result<Json<HealthResponse>> {
    let health = state.orchestrator.health().await?;
    Ok(Json(health))
}

// =============================================================================
// Run Management
// =============================================================================

pub async fn list_runs(State(state): State<Arc<AppState>>) -> Result<Json<Vec<RunSummary>>> {
    let runs = state.orchestrator.list_runs().await?;
    Ok(Json(runs))
}

pub async fn get_run(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<RunDetail>> {
    let run = state.orchestrator.get_run(&name).await?;
    Ok(Json(run))
}

pub async fn delete_run(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<StatusCode> {
    state.orchestrator.delete_run(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn pause_run(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<StatusCode> {
    state.orchestrator.pause_run(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn resume_run(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<ResumeRunRequest>,
) -> Result<StatusCode> {
    state
        .orchestrator
        .resume_run(&name, body.time_limit_minutes)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct DeliverResponse {
    branch: String,
}

pub async fn deliver_run(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<DeliverRunRequest>,
) -> Result<Json<DeliverResponse>> {
    let branch = state.orchestrator.deliver_run(&name, body.branch).await?;
    Ok(Json(DeliverResponse { branch }))
}

// =============================================================================
// Workers
// =============================================================================

pub async fn list_workers(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<Vec<Worker>>> {
    let workers = state.orchestrator.list_workers(&name).await?;
    Ok(Json(workers))
}

pub async fn restart_worker(
    State(state): State<Arc<AppState>>,
    Path((name, worker)): Path<(String, String)>,
) -> Result<StatusCode> {
    state.orchestrator.restart_worker(&name, &worker).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct LogResponse {
    content: String,
}

pub async fn get_worker_log(
    State(state): State<Arc<AppState>>,
    Path((name, worker)): Path<(String, String)>,
    Query(params): Query<WorkerLogParams>,
) -> Result<Json<LogResponse>> {
    let content = state
        .orchestrator
        .get_worker_log(&name, &worker, params.lines)
        .await?;
    Ok(Json(LogResponse { content }))
}

#[derive(Deserialize)]
pub struct WorkerEventParams {
    after_id: Option<i64>,
    limit: Option<i64>,
}

pub async fn get_worker_events(
    State(state): State<Arc<AppState>>,
    Path((name, worker)): Path<(String, String)>,
    Query(params): Query<WorkerEventParams>,
) -> Result<Json<WorkerEventsResponse>> {
    let events = state
        .orchestrator
        .get_worker_events(&name, &worker, params.after_id, params.limit)
        .await?;
    Ok(Json(events))
}

// =============================================================================
// Tasks
// =============================================================================

pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<Vec<Task>>> {
    let tasks = state.orchestrator.list_tasks(&name).await?;
    Ok(Json(tasks))
}

pub async fn add_task(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<AddTaskRequest>,
) -> Result<Json<Task>> {
    let task = state.orchestrator.add_task(&name, &body.content).await?;
    Ok(Json(task))
}

pub async fn delete_task(
    State(state): State<Arc<AppState>>,
    Path((name, task_id)): Path<(String, String)>,
) -> Result<StatusCode> {
    state.orchestrator.delete_task(&name, &task_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn complete_task(
    State(state): State<Arc<AppState>>,
    Path((name, task_id)): Path<(String, String)>,
) -> Result<StatusCode> {
    state.orchestrator.complete_task(&name, &task_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn reopen_task(
    State(state): State<Arc<AppState>>,
    Path((name, task_id)): Path<(String, String)>,
) -> Result<StatusCode> {
    state.orchestrator.reopen_task(&name, &task_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Messages
// =============================================================================

pub async fn list_threads(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<Vec<ThreadSummary>>> {
    let threads = state.orchestrator.list_threads(&name).await?;
    Ok(Json(threads))
}

pub async fn get_messages(
    State(state): State<Arc<AppState>>,
    Path((name, thread)): Path<(String, String)>,
) -> Result<Json<Vec<Message>>> {
    let messages = state.orchestrator.get_messages(&name, &thread).await?;
    Ok(Json(messages))
}

pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Path((name, thread)): Path<(String, String)>,
    Json(body): Json<SendMessageRequest>,
) -> Result<Json<Message>> {
    let message = state
        .orchestrator
        .send_message(&name, &thread, &body.content)
        .await?;
    Ok(Json(message))
}

// =============================================================================
// Evals
// =============================================================================

pub async fn list_evals(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<Vec<Eval>>> {
    let evals = state.orchestrator.list_evals(&name).await?;
    Ok(Json(evals))
}

// =============================================================================
// History
// =============================================================================

#[derive(Deserialize)]
pub struct HistoryParams {
    limit: Option<u32>,
}

pub async fn get_history(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<Vec<HistoryEntry>>> {
    let history = state.orchestrator.get_history(&name, params.limit).await?;
    Ok(Json(history))
}

// =============================================================================
// Config
// =============================================================================

pub async fn get_config(State(state): State<Arc<AppState>>) -> Result<Json<ConfigResponse>> {
    let config = state.orchestrator.get_config().await?;
    Ok(Json(config))
}
