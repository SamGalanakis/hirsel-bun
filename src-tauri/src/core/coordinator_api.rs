//! Coordinator HTTP API for remote workers.
//!
//! This is an axum-based HTTP wrapper around SQLiteState that allows remote workers
//! to access state via HTTP through SSH tunnels.
//!
//! The API only listens on localhost - remote workers connect via reverse SSH tunnel.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

use super::lifecycle::{LifecycleManager, LocalLifecycleManager};
use super::server::{
    eval_routes, message_routes, task_routes,
    worker_routes::{self, ReasonRequest, SuccessResponse},
};
use super::state::{SQLiteState, Status};

// =============================================================================
// Shared State
// =============================================================================

/// Shared state for the API handlers
pub struct ApiState {
    pub state: Arc<Mutex<SQLiteState>>,
    pub run_dir: PathBuf,
    pub run_name: String,
}

// =============================================================================
// Request/Response Models
// =============================================================================

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct StatusRequest {
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct TaskCreateRequest {
    pub task_id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub blocked_by: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct WorkerNameRequest {
    pub worker_name: String,
}

#[derive(Debug, Deserialize)]
pub struct WorkerCreateRequest {
    pub name: String,
    pub work_dir: String,
    #[serde(default = "default_location")]
    pub location: String,
}

fn default_location() -> String {
    "local".to_string()
}

#[derive(Debug, Deserialize)]
pub struct MessageCreateRequest {
    pub thread: String,
    pub sender: String,
    pub content: String,
    #[serde(default)]
    pub waiting: bool,
}

#[derive(Debug, Deserialize)]
pub struct MessageMarkReadRequest {
    pub reader: String,
    pub up_to_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct EvalCreateRequest {
    pub branch: String,
    pub eval_name: Option<String>,
    pub log_file: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EvalCompleteRequest {
    pub success: bool,
    pub feedback: String,
}

#[derive(Debug, Deserialize)]
pub struct TokensRequest {
    pub tokens: i64,
}

#[derive(Debug, Deserialize)]
pub struct LimitQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    100
}

// =============================================================================
// API Error Handler
// =============================================================================

pub struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(SuccessResponse::err(self.0.to_string())),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

type ApiResult<T> = Result<T, ApiError>;

// =============================================================================
// Router
// =============================================================================

/// Create the coordinator API router
pub fn create_router(state: Arc<Mutex<SQLiteState>>, run_dir: PathBuf, run_name: String) -> Router {
    let api_state = Arc::new(ApiState {
        state,
        run_dir,
        run_name,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Health
        .route("/health", get(health))
        // Run status
        .route("/status", get(get_status).post(set_status))
        // Tasks
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tasks/claimable", get(get_claimable_tasks))
        .route("/tasks/{task_id}", get(get_task).delete(delete_task))
        .route("/tasks/{task_id}/claim", post(claim_task))
        .route("/tasks/{task_id}/complete", post(complete_task))
        .route("/tasks/{task_id}/unclaim", post(unclaim_task))
        .route("/tasks/{task_id}/blocked", get(is_task_blocked))
        .route("/tasks/{task_id}/blockers", get(get_blockers))
        .route("/tasks/{task_id}/has_children", get(has_children))
        .route("/tasks/{task_id}/children", get(get_children))
        .route(
            "/tasks/{task_id}/pending_done",
            post(set_pending_done).delete(clear_pending_done),
        )
        .route("/tasks/{task_id}/reopen", post(reopen_task))
        .route("/tasks/{task_id}/tokens", post(set_task_tokens))
        // Workers
        .route("/workers", get(list_workers).post(create_worker))
        .route("/workers/active", get(get_active_workers))
        .route("/workers/all_done", get(all_workers_done))
        .route("/workers/pause_all", post(pause_all_workers))
        .route("/workers/resume_all", post(resume_all_workers))
        .route("/workers/{name}", get(get_worker))
        .route("/workers/{name}/update", post(update_worker))
        .route("/workers/{name}/claimed_task", get(get_claimed_task))
        .route("/workers/{name}/heartbeat", post(worker_heartbeat))
        // Messages
        .route("/messages/threads", get(list_threads))
        .route("/messages", post(create_message))
        .route("/messages/{thread}", get(get_messages))
        .route(
            "/messages/{thread}/unread/{reader}",
            get(get_unread_messages),
        )
        .route("/messages/unread/{reader}", get(get_all_unread))
        .route("/messages/{thread}/mark_read", post(mark_messages_read))
        // Evals
        .route("/evals", get(list_evals).post(create_eval))
        .route("/evals/running", get(get_running_eval))
        .route("/evals/cancel", post(cancel_running_evals))
        .route("/evals/{eval_id}", get(get_eval))
        .route("/evals/{eval_id}/complete", post(complete_eval))
        // Config
        .route("/config/request", get(get_request).post(set_request))
        .route(
            "/config/project_path",
            get(get_project_path).post(set_project_path),
        )
        .route(
            "/config/waiting_reason",
            get(get_waiting_reason).post(set_waiting_reason),
        )
        .route(
            "/config/human_in_the_loop",
            get(get_human_in_the_loop).post(set_human_in_the_loop),
        )
        .route("/config/summary", get(get_summary).post(set_summary))
        .route(
            "/config/worker_scale",
            get(get_worker_scale).post(set_worker_scale),
        )
        // Time tracking
        .route("/time/limit", get(get_time_limit).post(set_time_limit))
        .route("/time/started_at", get(get_started_at).post(set_started_at))
        .route("/time/info", get(get_time_info))
        .route("/time/expired", get(is_time_expired))
        .route(
            "/time/notification_pct",
            get(get_notification_pct).post(set_notification_pct),
        )
        .route("/time/clear", post(clear_time_tracking))
        // Iteration tracking
        .route("/iterations/count", get(get_iteration_count))
        .route("/iterations/increment", post(increment_iteration))
        .route(
            "/iterations/max",
            get(get_max_iterations).post(set_max_iterations),
        )
        // History
        .route("/history", get(get_history))
        // Lifecycle
        .route("/init", post(init_state))
        .with_state(api_state)
        .layer(cors)
}

// =============================================================================
// Health Handler
// =============================================================================

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

// =============================================================================
// Run Status Handlers
// =============================================================================

async fn get_status(State(api): State<Arc<ApiState>>) -> ApiResult<Json<StatusResponse>> {
    let state = api.state.lock().await;
    let status = state.status().map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(StatusResponse {
        status: status.to_string(),
    }))
}

async fn set_status(
    State(api): State<Arc<ApiState>>,
    Json(req): Json<StatusRequest>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let status = Status::from_str(&req.status)
        .ok_or_else(|| anyhow::anyhow!("Invalid status: {}", req.status))?;
    state
        .set_status(status)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

// =============================================================================
// Task Handlers - using shared logic from task_routes
// =============================================================================

async fn list_tasks(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let tasks = task_routes::list_tasks(&state).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "tasks": tasks })))
}

async fn create_task(
    State(api): State<Arc<ApiState>>,
    Json(req): Json<TaskCreateRequest>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let blocked_by_refs: Option<Vec<&str>> = req
        .blocked_by
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect());
    let blocked_by_slice: Option<&[&str]> = blocked_by_refs.as_deref();
    task_routes::create_task(
        &state,
        &req.task_id,
        &req.name,
        req.parent_id.as_deref(),
        blocked_by_slice,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn get_claimable_tasks(
    State(api): State<Arc<ApiState>>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let tasks = task_routes::get_claimable_tasks(&state).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "tasks": tasks })))
}

async fn get_task(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let task = task_routes::get_task(&state, &task_id).map_err(|e| anyhow::anyhow!("{}", e))?;
    match task {
        Some(t) => Ok(Json(serde_json::json!({ "task": t }))),
        None => Err(anyhow::anyhow!("Task '{}' not found", task_id).into()),
    }
}

async fn delete_task(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    task_routes::delete_task(&state, &task_id).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn claim_task(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
    Json(req): Json<WorkerNameRequest>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    task_routes::claim_task(&state, &task_id, &req.worker_name)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn complete_task(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
    Json(req): Json<WorkerNameRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    task_routes::complete_task(&state, &task_id, &req.worker_name)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "success": () })))
}

async fn unclaim_task(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
    Json(req): Json<WorkerNameRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    task_routes::unclaim_task(&state, &task_id, &req.worker_name)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "success": () })))
}

async fn is_task_blocked(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let blocked =
        task_routes::is_task_blocked(&state, &task_id).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "blocked": blocked })))
}

async fn get_blockers(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let blockers =
        task_routes::get_blockers(&state, &task_id).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "blockers": blockers })))
}

async fn has_children(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let has = task_routes::has_children(&state, &task_id).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "has_children": has })))
}

async fn get_children(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let children =
        task_routes::get_children(&state, &task_id).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "children": children })))
}

async fn set_pending_done(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    task_routes::set_pending_done(&state, &task_id).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn clear_pending_done(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    task_routes::clear_pending_done(&state, &task_id).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn reopen_task(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    task_routes::reopen_task(&state, &task_id).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "success": () })))
}

async fn set_task_tokens(
    State(api): State<Arc<ApiState>>,
    Path(task_id): Path<String>,
    Json(req): Json<TokensRequest>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    task_routes::set_task_tokens(&state, &task_id, req.tokens)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

// =============================================================================
// Worker Handlers - using shared logic from worker_routes
// =============================================================================

async fn list_workers(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let workers = worker_routes::list_workers(&state).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "workers": workers })))
}

async fn create_worker(
    State(api): State<Arc<ApiState>>,
    Json(req): Json<WorkerCreateRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let worker = worker_routes::create_worker(&state, &req.name, &req.work_dir, &req.location)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "worker": worker })))
}

async fn get_active_workers(
    State(api): State<Arc<ApiState>>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let workers =
        worker_routes::list_active_workers(&state).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "workers": workers })))
}

async fn all_workers_done(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let all_done = worker_routes::all_workers_done(&state).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "all_done": all_done })))
}

async fn pause_all_workers(
    State(api): State<Arc<ApiState>>,
    Json(req): Json<ReasonRequest>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    worker_routes::pause_all_workers(&state, &req.reason).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn resume_all_workers(State(api): State<Arc<ApiState>>) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    worker_routes::resume_all_workers(&state).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn get_worker(
    State(api): State<Arc<ApiState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let worker = worker_routes::get_worker(&state, &name).map_err(|e| anyhow::anyhow!("{}", e))?;
    match worker {
        Some(w) => Ok(Json(serde_json::json!({ "worker": w }))),
        None => Err(anyhow::anyhow!("Worker '{}' not found", name).into()),
    }
}

async fn update_worker(
    State(api): State<Arc<ApiState>>,
    Path(name): Path<String>,
    Json(req): Json<worker_routes::UpdateWorkerRequest>,
) -> ApiResult<Json<SuccessResponse>> {
    // Create lifecycle manager for handling status transitions
    let agent_command = crate::cli::config::get_agent_command();
    let lifecycle =
        LocalLifecycleManager::new(&api.run_name, api.run_dir.clone(), agent_command).ok();

    let state = api.state.lock().await;
    worker_routes::update_worker(
        &state,
        &name,
        &req,
        lifecycle.as_ref().map(|l| l as &dyn LifecycleManager),
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(Json(SuccessResponse::ok()))
}

async fn get_claimed_task(
    State(api): State<Arc<ApiState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let task =
        worker_routes::get_claimed_task(&state, &name).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "task": task })))
}

async fn worker_heartbeat(
    State(api): State<Arc<ApiState>>,
    Path(name): Path<String>,
) -> ApiResult<Json<StatusResponse>> {
    let state = api.state.lock().await;
    let status =
        worker_routes::worker_heartbeat(&state, &name).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(StatusResponse {
        status: status.to_string(),
    }))
}

// =============================================================================
// Message Handlers - using shared logic from message_routes
// =============================================================================

async fn list_threads(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let threads = message_routes::list_threads(&state).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "threads": threads })))
}

async fn create_message(
    State(api): State<Arc<ApiState>>,
    Json(req): Json<MessageCreateRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let id =
        message_routes::create_message(&state, &req.thread, &req.sender, &req.content, req.waiting)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

async fn get_messages(
    State(api): State<Arc<ApiState>>,
    Path(thread): Path<String>,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let messages = message_routes::get_messages(&state, &thread, query.limit)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "messages": messages })))
}

async fn get_unread_messages(
    State(api): State<Arc<ApiState>>,
    Path((thread, reader)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let messages = message_routes::get_unread_messages(&state, &thread, &reader)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "messages": messages })))
}

async fn get_all_unread(
    State(api): State<Arc<ApiState>>,
    Path(reader): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let messages =
        message_routes::get_all_unread(&state, &reader).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "messages": messages })))
}

async fn mark_messages_read(
    State(api): State<Arc<ApiState>>,
    Path(thread): Path<String>,
    Json(req): Json<MessageMarkReadRequest>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    message_routes::mark_messages_read(&state, &thread, &req.reader, req.up_to_id)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

// =============================================================================
// Eval Handlers - using shared logic from eval_routes
// =============================================================================

async fn list_evals(
    State(api): State<Arc<ApiState>>,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let evals =
        eval_routes::list_evals(&state, query.limit).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "evals": evals })))
}

async fn create_eval(
    State(api): State<Arc<ApiState>>,
    Json(req): Json<EvalCreateRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let id = eval_routes::start_eval(
        &state,
        &req.branch,
        req.eval_name.as_deref(),
        req.log_file.as_deref(),
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

async fn get_running_eval(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let eval = eval_routes::get_running_eval(&state).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "eval": eval })))
}

async fn cancel_running_evals(
    State(api): State<Arc<ApiState>>,
    Json(req): Json<ReasonRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let count = eval_routes::cancel_running_evals(&state, &req.reason)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "count": count })))
}

async fn get_eval(
    State(api): State<Arc<ApiState>>,
    Path(eval_id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let eval = eval_routes::get_eval(&state, eval_id).map_err(|e| anyhow::anyhow!("{}", e))?;
    match eval {
        Some(e) => Ok(Json(serde_json::json!({ "eval": e }))),
        None => Err(anyhow::anyhow!("Eval {} not found", eval_id).into()),
    }
}

async fn complete_eval(
    State(api): State<Arc<ApiState>>,
    Path(eval_id): Path<i64>,
    Json(req): Json<EvalCompleteRequest>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    eval_routes::complete_eval(&state, eval_id, req.success, &req.feedback)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

// =============================================================================
// Config Handlers
// =============================================================================

async fn get_request(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let request = state.get_request().map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "request": request })))
}

async fn set_request(
    State(api): State<Arc<ApiState>>,
    Json(data): Json<serde_json::Value>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let request = data.get("request").and_then(|v| v.as_str());
    state
        .set_request(request)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn get_project_path(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let path = state
        .get_project_path()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "project_path": path })))
}

async fn set_project_path(
    State(api): State<Arc<ApiState>>,
    Json(data): Json<serde_json::Value>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let path = data
        .get("project_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing project_path"))?;
    state
        .set_project_path(path)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn get_waiting_reason(
    State(api): State<Arc<ApiState>>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let reason = state
        .get_waiting_reason()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "waiting_reason": reason })))
}

async fn set_waiting_reason(
    State(api): State<Arc<ApiState>>,
    Json(data): Json<serde_json::Value>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let reason = data.get("reason").and_then(|v| v.as_str());
    state
        .set_waiting_reason(reason)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn get_human_in_the_loop(
    State(api): State<Arc<ApiState>>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let enabled = state
        .get_human_in_the_loop()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "enabled": enabled })))
}

async fn set_human_in_the_loop(
    State(api): State<Arc<ApiState>>,
    Json(data): Json<serde_json::Value>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let enabled = data
        .get("enabled")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| anyhow::anyhow!("Missing enabled"))?;
    state
        .set_human_in_the_loop(enabled)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn get_summary(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let summary = state.get_summary().map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "summary": summary })))
}

async fn set_summary(
    State(api): State<Arc<ApiState>>,
    Json(data): Json<serde_json::Value>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let summary = data
        .get("summary")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing summary"))?;
    state
        .set_summary(summary)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn get_worker_scale(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let scale = state
        .get_worker_scale()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "worker_scale": scale })))
}

async fn set_worker_scale(
    State(api): State<Arc<ApiState>>,
    Json(data): Json<serde_json::Value>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let scale = data
        .get("scale")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing scale"))?;
    state
        .set_worker_scale(scale)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

// =============================================================================
// Time Tracking Handlers
// =============================================================================

async fn get_time_limit(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let minutes = state
        .get_time_limit_minutes()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "minutes": minutes })))
}

async fn set_time_limit(
    State(api): State<Arc<ApiState>>,
    Json(data): Json<serde_json::Value>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let minutes = data.get("minutes").and_then(|v| v.as_i64());
    state
        .set_time_limit_minutes(minutes)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn get_started_at(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let started_at = state
        .get_started_at()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "started_at": started_at })))
}

async fn set_started_at(
    State(api): State<Arc<ApiState>>,
    Json(data): Json<serde_json::Value>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let timestamp = data.get("timestamp").and_then(|v| v.as_str());
    state
        .set_started_at(timestamp)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn get_time_info(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let info = state
        .get_time_info()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    // Serialize TimeInfo - note that DateTime fields may need special handling
    Ok(Json(serde_json::json!({ "info": info })))
}

async fn is_time_expired(State(api): State<Arc<ApiState>>) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let expired = state
        .is_time_expired()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "expired": expired })))
}

async fn get_notification_pct(
    State(api): State<Arc<ApiState>>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let pct = state
        .get_last_time_notification_pct()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "pct": pct })))
}

async fn set_notification_pct(
    State(api): State<Arc<ApiState>>,
    Json(data): Json<serde_json::Value>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let pct = data
        .get("pct")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("Missing pct"))?;
    state
        .set_last_time_notification_pct(pct)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

async fn clear_time_tracking(State(api): State<Arc<ApiState>>) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    state
        .clear_time_tracking()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

// =============================================================================
// Iteration Tracking Handlers
// =============================================================================

async fn get_iteration_count(
    State(api): State<Arc<ApiState>>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let count = state
        .get_iteration_count()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "count": count })))
}

async fn increment_iteration(
    State(api): State<Arc<ApiState>>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let count = state
        .increment_iteration()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "count": count })))
}

async fn get_max_iterations(
    State(api): State<Arc<ApiState>>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let max = state
        .get_max_iterations()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "max": max })))
}

async fn set_max_iterations(
    State(api): State<Arc<ApiState>>,
    Json(data): Json<serde_json::Value>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let max = data.get("max").and_then(|v| v.as_i64());
    state
        .set_max_iterations(max)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

// =============================================================================
// History Handler
// =============================================================================

async fn get_history(
    State(api): State<Arc<ApiState>>,
    Query(query): Query<LimitQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let state = api.state.lock().await;
    let history = state
        .get_history(query.limit)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(serde_json::json!({ "history": history })))
}

// =============================================================================
// Lifecycle Handler
// =============================================================================

async fn init_state(
    State(api): State<Arc<ApiState>>,
    Json(data): Json<serde_json::Value>,
) -> ApiResult<Json<SuccessResponse>> {
    let state = api.state.lock().await;
    let project_path = data.get("project_path").and_then(|v| v.as_str());
    state
        .init_state(project_path)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(SuccessResponse::ok()))
}

// =============================================================================
// Server
// =============================================================================

/// Coordinator server that manages the HTTP API
pub struct CoordinatorServer {
    state: Arc<Mutex<SQLiteState>>,
    host: String,
    port: u16,
    run_dir: PathBuf,
    run_name: String,
    staging_path: Option<PathBuf>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl CoordinatorServer {
    /// Create a new coordinator server
    pub fn new(
        state: SQLiteState,
        host: impl Into<String>,
        port: u16,
        run_dir: PathBuf,
        run_name: String,
        staging_path: Option<PathBuf>,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
            host: host.into(),
            port,
            run_dir,
            run_name,
            staging_path,
            handle: None,
        }
    }

    /// Start the API server
    pub async fn start(&mut self) -> anyhow::Result<()> {
        let mut router = create_router(
            self.state.clone(),
            self.run_dir.clone(),
            self.run_name.clone(),
        );

        // Mount git HTTP server if staging path provided
        // This allows workers to push/pull changes via git
        if let Some(ref staging_path) = self.staging_path {
            tracing::info!(
                "Mounting git HTTP server at /git/{} (repo: {})",
                self.run_name,
                staging_path.display()
            );
            let git_router = super::git_http::create_git_router(staging_path.clone());
            router = router.nest(&format!("/git/{}", self.run_name), git_router);
        }

        let addr: SocketAddr = format!("{}:{}", self.host, self.port).parse()?;
        tracing::info!("Coordinator API starting on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.ok();
        });

        self.handle = Some(handle);
        Ok(())
    }

    /// Stop the API server
    pub fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
            tracing::info!("Coordinator API stopped");
        }
    }

    /// Get the API URL
    pub fn url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    /// Get the port
    pub fn port(&self) -> u16 {
        self.port
    }
}
