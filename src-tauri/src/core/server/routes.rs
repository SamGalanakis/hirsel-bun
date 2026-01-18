//! API route handlers

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, StatusCode},
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
    AddTaskRequest, CreateRunRequest, CreateRunResponse, DeliverRunRequest, HealthResponse,
    Orchestrator, OrchestratorError, ResumeRunRequest, SendMessageRequest, SpawnWorkersRequest,
    SpawnWorkersResponse, WorkerLogParams,
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

pub async fn create_run(
    State(_state): State<Arc<AppState>>,
    Json(body): Json<CreateRunRequest>,
) -> Result<Json<CreateRunResponse>> {
    use crate::cli::go::slugify;
    use crate::core::chats::{
        create_default_group_chat, create_default_user_chat, create_learnings_thread,
        create_worker_chat,
    };
    use crate::core::config;
    use crate::core::files::Files;
    use crate::core::names;
    use crate::core::state::{SQLiteState, Status};

    // Slugify and validate run name
    let run_name = slugify(&body.name);
    if run_name.len() > 50 {
        return Err(OrchestratorError::InvalidOperation(format!(
            "Run name too long (max 50 chars): {}...",
            &run_name[..50]
        )));
    }

    // Get run directory
    let run_dir = config::run_dir(&run_name);
    if run_dir.exists() {
        let db_path = run_dir.join("hirsel.db");
        if db_path.exists() {
            if let Ok(existing_state) = SQLiteState::new(db_path) {
                if let Ok(status) = existing_state.status() {
                    if status == Status::Working || status == Status::Eval {
                        return Err(OrchestratorError::InvalidOperation(format!(
                            "Run '{}' already exists and is active",
                            run_name
                        )));
                    }
                }
            }
        }
        // Clean up old run
        let _ = std::fs::remove_dir_all(&run_dir);
    }

    // Create run directory
    std::fs::create_dir_all(&run_dir)
        .map_err(|e| OrchestratorError::Other(format!("Failed to create run directory: {}", e)))?;

    // Initialize Files
    let files = Files::new(run_dir.clone());
    files
        .init_dirs()
        .map_err(|e| OrchestratorError::Other(format!("Failed to init dirs: {}", e)))?;

    // Write spec file
    std::fs::write(files.spec(), &body.spec)
        .map_err(|e| OrchestratorError::Other(format!("Failed to write spec: {}", e)))?;

    // Write eval file if provided
    if let Some(ref eval_content) = body.eval {
        std::fs::write(run_dir.join("eval.md"), eval_content)
            .map_err(|e| OrchestratorError::Other(format!("Failed to write eval: {}", e)))?;
    }

    // Initialize bootstrap tasks.md
    std::fs::write(
        run_dir.join("tasks.md"),
        "# Tasks\n\n| ID | Status | Worker | Name |\n|----|--------|--------|------|\n| scope | TODO | | Read spec, create exploration tasks |\n",
    ).map_err(|e| OrchestratorError::Other(format!("Failed to write tasks.md: {}", e)))?;

    // Create tasks detail folder
    let tasks_dir = run_dir.join("tasks");
    std::fs::create_dir_all(&tasks_dir)
        .map_err(|e| OrchestratorError::Other(format!("Failed to create tasks dir: {}", e)))?;
    std::fs::write(tasks_dir.join("scope.md"), "")
        .map_err(|e| OrchestratorError::Other(format!("Failed to write scope.md: {}", e)))?;

    // Initialize SQLite state
    let db_path = run_dir.join("hirsel.db");
    let sqlite_state = SQLiteState::new(db_path)
        .map_err(|e| OrchestratorError::Other(format!("Failed to create state: {}", e)))?;
    sqlite_state
        .init_state(None)
        .map_err(|e| OrchestratorError::Other(format!("Failed to init state: {}", e)))?;

    // Set run properties
    sqlite_state
        .set_request(Some(&body.spec))
        .map_err(|e| OrchestratorError::Other(format!("Failed to set request: {}", e)))?;

    if let Some(scale) = body.worker_scale {
        sqlite_state
            .set_worker_scale(&scale.to_string())
            .map_err(|e| OrchestratorError::Other(format!("Failed to set worker scale: {}", e)))?;
    }

    if let Some(limit) = body.time_limit_minutes {
        sqlite_state
            .set_time_limit_minutes(Some(limit as i64))
            .map_err(|e| OrchestratorError::Other(format!("Failed to set time limit: {}", e)))?;
    }

    if let Some(max_iter) = body.max_iterations {
        sqlite_state
            .set_max_iterations(Some(max_iter as i64))
            .map_err(|e| {
                OrchestratorError::Other(format!("Failed to set max iterations: {}", e))
            })?;
    }

    if let Some(hitl) = body.human_in_the_loop {
        sqlite_state
            .set_human_in_the_loop(hitl)
            .map_err(|e| OrchestratorError::Other(format!("Failed to set HITL: {}", e)))?;
    }

    // Note: runner is used from server config, not stored per-run

    // Add initial scope task
    let _ = sqlite_state.add_task("scope", "Read spec, create exploration tasks", None, None);

    // Set status to Draft (not spawning workers yet)
    sqlite_state
        .set_status(Status::Draft)
        .map_err(|e| OrchestratorError::Other(format!("Failed to set status: {}", e)))?;

    // Create initial worker name (for pre-claiming scope task)
    let first_worker_name = names::generate_worker_name();
    let _ = sqlite_state.claim_task("scope", &first_worker_name);

    // Determine multi-worker mode from scale
    let max_scale = body.worker_scale.unwrap_or(1);
    let is_multi_worker = max_scale > 1;

    // Create chat files
    let chats_dir = files.chats_dir();
    create_default_user_chat(&chats_dir)
        .map_err(|e| OrchestratorError::Other(format!("Failed to create user chat: {}", e)))?;

    if is_multi_worker {
        create_default_group_chat(
            &chats_dir,
            &[first_worker_name.clone()],
            Some(&first_worker_name),
        )
        .map_err(|e| OrchestratorError::Other(format!("Failed to create group chat: {}", e)))?;
    }

    create_learnings_thread(&chats_dir, &[first_worker_name.clone()])
        .map_err(|e| OrchestratorError::Other(format!("Failed to create learnings: {}", e)))?;

    create_worker_chat(&chats_dir, &first_worker_name)
        .map_err(|e| OrchestratorError::Other(format!("Failed to create worker chat: {}", e)))?;

    // Register initial worker (without work_dir - will be set after files upload)
    sqlite_state
        .add_worker(&first_worker_name, "", "remote")
        .map_err(|e| OrchestratorError::Other(format!("Failed to register worker: {}", e)))?;

    tracing::info!(
        "Created run '{}' with initial worker '{}'",
        run_name,
        first_worker_name
    );

    Ok(Json(CreateRunResponse {
        name: run_name.clone(),
        run_dir: run_dir.to_string_lossy().to_string(),
        files_url: format!("/api/runs/{}/files", run_name),
    }))
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
// File Transfer
// =============================================================================

/// Upload working directory as tarball
///
/// Accepts a gzipped tar archive of the project files.
/// Extracts to the run's work directory for workers to use.
pub async fn upload_files(
    State(_state): State<Arc<AppState>>,
    Path(name): Path<String>,
    body: Bytes,
) -> Result<StatusCode> {
    use crate::core::config;
    use flate2::read::GzDecoder;
    use tar::Archive;

    let run_dir = config::run_dir(&name);
    if !run_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    // Create work directory
    let work_dir = run_dir.join("work");
    std::fs::create_dir_all(&work_dir)
        .map_err(|e| OrchestratorError::Other(format!("Failed to create work dir: {}", e)))?;

    // Extract tarball
    let decoder = GzDecoder::new(&body[..]);
    let mut archive = Archive::new(decoder);

    archive
        .unpack(&work_dir)
        .map_err(|e| OrchestratorError::Other(format!("Failed to extract tarball: {}", e)))?;

    tracing::info!(
        "Uploaded files for run '{}' to {}",
        name,
        work_dir.display()
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Download working directory as tarball
///
/// Returns a gzipped tar archive of the run's work directory.
/// Workers call this to get project files.
pub async fn download_files(
    State(_state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> std::result::Result<impl IntoResponse, OrchestratorError> {
    use crate::core::config;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use tar::Builder;

    let run_dir = config::run_dir(&name);
    if !run_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let work_dir = run_dir.join("work");
    if !work_dir.exists() {
        return Err(OrchestratorError::Other(
            "Work directory not found. Upload files first.".into(),
        ));
    }

    // Create tarball
    let mut buffer = Vec::new();
    {
        let encoder = GzEncoder::new(&mut buffer, Compression::fast());
        let mut builder = Builder::new(encoder);

        builder
            .append_dir_all(".", &work_dir)
            .map_err(|e| OrchestratorError::Other(format!("Failed to create tarball: {}", e)))?;

        builder
            .into_inner()
            .map_err(|e| OrchestratorError::Other(format!("Failed to finish tarball: {}", e)))?
            .finish()
            .map_err(|e| OrchestratorError::Other(format!("Failed to compress: {}", e)))?;
    }

    tracing::info!("Serving files for run '{}' ({} bytes)", name, buffer.len());

    Ok(([(header::CONTENT_TYPE, "application/gzip")], buffer))
}

/// Spawn workers for a run
///
/// Creates and starts the specified number of workers.
/// The run must have files uploaded first.
pub async fn spawn_workers(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<SpawnWorkersRequest>,
) -> Result<Json<SpawnWorkersResponse>> {
    use crate::cli::config::get_agent_command;
    use crate::core::chats::{create_default_group_chat, create_worker_chat};
    use crate::core::config;
    use crate::core::files::Files;
    use crate::core::names;
    use crate::core::runner::{Runner, RunnerConfig, WorkerSpawnConfig};
    use crate::core::state::{SQLiteState, Status, WorkerUpdate};

    let run_dir = config::run_dir(&name);
    if !run_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name.clone()));
    }

    let work_dir = run_dir.join("work");
    if !work_dir.exists() {
        return Err(OrchestratorError::InvalidOperation(
            "Work directory not found. Upload files first.".into(),
        ));
    }

    // Open state
    let db_path = run_dir.join("hirsel.db");
    let sqlite_state = SQLiteState::new(db_path)
        .map_err(|e| OrchestratorError::Other(format!("Failed to open state: {}", e)))?;

    // Check run status
    let status = sqlite_state
        .status()
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    if status != Status::Draft && status != Status::Paused {
        return Err(OrchestratorError::InvalidOperation(format!(
            "Cannot spawn workers for run in '{}' status",
            status
        )));
    }

    // Get existing workers
    let existing_workers = sqlite_state
        .get_workers()
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    let existing_names: Vec<String> = existing_workers.iter().map(|w| w.name.clone()).collect();
    let is_multi_worker = existing_workers.len() + body.count as usize > 1;

    // Generate names for new workers
    let mut new_worker_names: Vec<String> = names::generate_unique_names(body.count as usize)
        .into_iter()
        .filter(|n| !existing_names.contains(n))
        .take(body.count as usize)
        .collect();

    // Need more names if we didn't get enough unique ones
    if new_worker_names.len() < body.count as usize {
        let mut all_used: std::collections::HashSet<String> =
            existing_names.iter().cloned().collect();
        all_used.extend(new_worker_names.iter().cloned());

        while new_worker_names.len() < body.count as usize {
            let name = names::generate_worker_name();
            if !all_used.contains(&name) {
                all_used.insert(name.clone());
                new_worker_names.push(name);
            }
        }
    }

    // Determine leader and teammates
    let leader_name = existing_workers.first().map(|w| w.name.clone());
    let all_worker_names: Vec<String> = existing_names
        .iter()
        .chain(new_worker_names.iter())
        .cloned()
        .collect();

    // Get runner config from server config
    let config = state.orchestrator.config();
    let runner_name = config.default_runner.clone().unwrap_or_default();
    let runner_config = config
        .get_runner(&runner_name)
        .unwrap_or(RunnerConfig::Local);

    // Get agent command
    let agent_command = get_agent_command();
    let files = Files::new(run_dir.clone());
    let spec_path = files.spec();
    let chats_dir = files.chats_dir();

    // Create runner
    let runner: Box<dyn Runner> = match &runner_config {
        RunnerConfig::Local => Box::new(crate::core::runner::LocalRunner::new()),
        RunnerConfig::Sprite(cfg) => Box::new(crate::core::runner::SpriteRunner::new(cfg.clone())),
        RunnerConfig::Ssh(cfg) => Box::new(crate::core::runner::SshRunner::new(cfg.clone())),
    };

    // Ensure group chat exists for multi-worker
    if is_multi_worker && !chats_dir.join("group.md").exists() {
        let _ = create_default_group_chat(&chats_dir, &all_worker_names, leader_name.as_deref());
    }

    // Spawn workers
    let mut spawned_workers = Vec::new();
    let rt = tokio::runtime::Handle::current();

    for worker_name in &new_worker_names {
        // Create worker chat
        let _ = create_worker_chat(&chats_dir, worker_name);

        // Register worker
        sqlite_state
            .add_worker(worker_name, work_dir.to_str().unwrap_or("."), "remote")
            .map_err(|e| OrchestratorError::Other(format!("Failed to register worker: {}", e)))?;

        // Build teammates list (exclude self)
        let teammates: Option<Vec<String>> = if is_multi_worker {
            Some(
                all_worker_names
                    .iter()
                    .filter(|t| *t != worker_name)
                    .cloned()
                    .collect(),
            )
        } else {
            None
        };

        let spawn_config = WorkerSpawnConfig {
            run_name: name.clone(),
            worker_name: worker_name.clone(),
            work_dir: work_dir.clone(),
            run_dir: run_dir.clone(),
            spec_path: spec_path.clone(),
            agent_command: agent_command.clone(),
            is_leader: false, // Only first worker is leader
            leader_name: leader_name.clone(),
            teammates,
            resume_session_id: None,
            env_vars: None,
            coordinator_url: None,
            project_url: None,
        };

        match rt.block_on(runner.spawn(&spawn_config)) {
            Ok(result) => {
                // Update worker with PID
                let pid = result.pid.map(|p| p as i64);
                let _ = sqlite_state.update_worker(
                    worker_name,
                    WorkerUpdate {
                        pid,
                        status: Some(crate::core::state::WorkerStatus::Working),
                        ..Default::default()
                    },
                );
                spawned_workers.push(worker_name.clone());
                tracing::info!(
                    "Spawned worker '{}' (runner_id: {})",
                    worker_name,
                    result.handle.runner_id
                );
            }
            Err(e) => {
                tracing::warn!("Failed to spawn worker '{}': {}", worker_name, e);
            }
        }
    }

    // Update run status to Working if we spawned any workers
    if !spawned_workers.is_empty() {
        sqlite_state
            .set_status(Status::Working)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        sqlite_state
            .set_started_at(None)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
    }

    Ok(Json(SpawnWorkersResponse {
        workers: spawned_workers,
    }))
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
