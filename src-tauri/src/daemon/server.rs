//! TCP server for the hirsel daemon
//!
//! Listens on a TCP port (default 19700, configurable via HIRSEL_DAEMON_PORT) for HTTP requests.
//! Local CLI/GUI connects via localhost, remote workers via Docker host or SSH tunnels.

use anyhow::{anyhow, Result};
use axum::{
    routing::{delete, get, post},
    Router,
};
use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

use crate::core::config::{paths::hirsel_dir, Config};
use crate::core::orchestrator::{LocalOrchestrator, Orchestrator};
use crate::core::server::{gyp, AppState};

use super::lifecycle;

/// Default TCP port for the daemon HTTP server
pub const DEFAULT_TCP_PORT: u16 = 19700;

/// Daemon configuration
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Idle timeout in seconds (daemon exits if no active runs for this long)
    /// Set to 0 to disable auto-exit
    pub idle_timeout_secs: u64,
    /// TCP port for HTTP access (for SSH reverse tunnels)
    /// Set to 0 to disable TCP listener
    pub tcp_port: u16,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            idle_timeout_secs: 300, // 5 minutes
            tcp_port: super::get_daemon_port(),
        }
    }
}

/// Check if port is already in use
fn check_port_conflict(port: u16) -> Result<()> {
    use std::net::TcpStream;
    use std::time::Duration;

    let addr = format!("127.0.0.1:{}", port);
    if TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_millis(100)).is_ok() {
        // Port is in use - could be another hirsel daemon or something else
        return Err(anyhow!(
            "Port {} is already in use.\n\
             This may be another hirsel daemon (for a different HIRSEL_ROOT).\n\
             Set HIRSEL_DAEMON_PORT to use a different port.\n\
             Current HIRSEL_ROOT: {}",
            port,
            hirsel_dir().display()
        ));
    }
    Ok(())
}

/// Start the daemon server on TCP
pub async fn start_daemon(config: DaemonConfig) -> Result<()> {
    // Check for port conflicts before starting
    check_port_conflict(config.tcp_port)?;

    let pid_path = super::pid_path();

    // Ensure the parent directory exists
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Write PID file with binary path for mismatch detection
    let pid = std::process::id();
    let exe_path = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    std::fs::write(&pid_path, format!("{}\n{}", pid, exe_path))?;

    // Create cleanup handler for graceful shutdown
    let pid_path_clone = pid_path.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("[Daemon] Received shutdown signal");
        cleanup_pid_file(&pid_path_clone);
        std::process::exit(0);
    });

    // Load config and create orchestrator
    let (hirsel_config, warnings) =
        Config::load().map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))?;
    for warning in warnings {
        tracing::warn!("{}", warning);
    }

    let orchestrator = LocalOrchestrator::new(hirsel_config.clone());
    let app_config = Arc::new(RwLock::new(hirsel_config));
    let state = Arc::new(AppState {
        orchestrator,
        config: app_config,
    });

    // Start lifecycle polling loop
    let lifecycle_state = state.clone();
    let daemon_config = config.clone();
    tokio::spawn(async move {
        lifecycle::run_polling_loop(lifecycle_state, daemon_config).await;
    });

    // Build router (same routes as core/server but no auth for local daemon)
    let gyp_state = Arc::new(gyp::GypState::new());
    let router = build_router(state, gyp_state);

    // Bind to TCP port
    // Use 0.0.0.0 to allow connections from Docker containers via host.docker.internal
    let tcp_addr = format!("0.0.0.0:{}", config.tcp_port);
    let tcp_listener = TcpListener::bind(&tcp_addr).await?;
    tracing::info!("[Daemon] Listening on TCP: {}", tcp_addr);
    tracing::info!("[Daemon] PID: {}", pid);

    // TCP accept loop (main loop)
    loop {
        match tcp_listener.accept().await {
            Ok((stream, addr)) => {
                let router = router.clone();
                tokio::spawn(async move {
                    tracing::debug!("[Daemon] TCP connection from {}", addr);
                    let io = hyper_util::rt::TokioIo::new(stream);
                    let service =
                        hyper_util::service::TowerToHyperService::new(router.into_service());
                    if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(io, service)
                    .await
                    {
                        tracing::warn!("[Daemon] TCP connection error: {}", e);
                    }
                });
            }
            Err(e) => {
                tracing::warn!("[Daemon] TCP accept error: {}", e);
            }
        }
    }
}

/// Build the axum router with all routes
fn build_router(state: Arc<AppState>, gyp_state: Arc<gyp::GypState>) -> Router {
    use crate::core::server::routes;

    // Build Gyp chat routes with separate state
    let gyp_routes = Router::new()
        .route(
            "/api/gyp/sessions",
            get(gyp::list_sessions).post(gyp::start_session),
        )
        .route("/api/gyp/sessions/{id}", delete(gyp::stop_session))
        .route("/api/gyp/sessions/{id}/messages", post(gyp::send_message))
        .route(
            "/api/gyp/sessions/{id}/permission",
            post(gyp::respond_permission),
        )
        .route("/api/gyp/sessions/{id}/events", get(gyp::session_events))
        .with_state(gyp_state);

    // Build the main router (no auth layer for local daemon - localhost only)
    Router::new()
        // Health check
        .route("/health", get(routes::health))
        // Daemon-specific routes
        .route("/daemon/stop", post(daemon_stop))
        .route("/daemon/status", get(daemon_status))
        // Run management
        .route("/api/runs", get(routes::list_runs).post(routes::create_run))
        .route(
            "/api/runs/{name}",
            get(routes::get_run).delete(routes::delete_run),
        )
        .route(
            "/api/runs/{name}/files",
            get(routes::download_files).post(routes::upload_files),
        )
        .route("/api/runs/{name}/spawn", post(routes::spawn_workers))
        .route("/api/runs/{name}/pause", post(routes::pause_run))
        .route("/api/runs/{name}/resume", post(routes::resume_run))
        .route("/api/runs/{name}/deliver", post(routes::deliver_run))
        // Workers
        .route("/api/runs/{name}/workers", get(routes::list_workers))
        .route(
            "/api/runs/{name}/workers/{worker}/restart",
            post(routes::restart_worker),
        )
        .route(
            "/api/runs/{name}/workers/{worker}/spawn",
            post(routes::spawn_single_worker),
        )
        .route(
            "/api/runs/{name}/workers/{worker}/resume",
            post(routes::resume_worker),
        )
        .route(
            "/api/runs/{name}/workers/{worker}/events",
            get(routes::get_worker_events),
        )
        // Tasks
        .route(
            "/api/runs/{name}/tasks",
            get(routes::list_tasks).post(routes::add_task),
        )
        .route(
            "/api/runs/{name}/tasks/{task_id}",
            delete(routes::delete_task),
        )
        .route(
            "/api/runs/{name}/tasks/{task_id}/complete",
            post(routes::complete_task),
        )
        .route(
            "/api/runs/{name}/tasks/{task_id}/reopen",
            post(routes::reopen_task),
        )
        // Threads and messages
        .route("/api/runs/{name}/threads", get(routes::list_threads))
        .route(
            "/api/runs/{name}/threads/{thread}/messages",
            get(routes::get_messages).post(routes::send_message),
        )
        // Evals
        .route("/api/runs/{name}/evals", get(routes::list_evals))
        // History
        .route("/api/runs/{name}/history", get(routes::get_history))
        // Assets
        .route("/api/runs/{name}/assets", post(gyp::upload_asset))
        .route("/api/runs/{name}/assets-path", get(gyp::get_assets_path))
        // Config
        .route("/api/config", get(routes::get_config))
        // Per-run config endpoints for workers
        .route(
            "/api/runs/{run}/config/human_in_the_loop",
            get(get_run_hitl),
        )
        .route("/api/runs/{run}/config/request", get(get_run_request))
        .route(
            "/api/runs/{run}/config/project_path",
            get(get_run_project_path),
        )
        .route(
            "/api/runs/{run}/config/waiting_reason",
            get(get_run_waiting_reason).post(set_run_waiting_reason),
        )
        // Worker state endpoints for workers (internal API)
        .route("/api/runs/{run}/workers/list", get(list_workers))
        .route("/api/runs/{run}/workers/active", get(list_active_workers))
        .route("/api/runs/{run}/workers/all_done", get(all_workers_done))
        .route("/api/runs/{run}/workers/{worker}", get(get_worker))
        .route(
            "/api/runs/{run}/workers/{worker}/update",
            post(update_worker),
        )
        .route(
            "/api/runs/{run}/workers/{worker}/heartbeat",
            post(worker_heartbeat),
        )
        .route(
            "/api/runs/{run}/workers/{worker}/claimed_task",
            get(get_worker_claimed_task),
        )
        // Merge Gyp routes
        .merge(gyp_routes)
        // CORS for browser-based clients
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

/// Clean up PID file
pub(crate) fn cleanup_pid_file(pid_path: &Path) {
    if pid_path.exists() {
        let _ = std::fs::remove_file(pid_path);
    }
}

// =============================================================================
// Daemon-specific routes
// =============================================================================

use axum::{extract::State, Json};
use serde::Serialize;

#[derive(Serialize)]
struct DaemonStatus {
    running: bool,
    pid: u32,
    tcp_port: u16,
    hirsel_root: String,
    uptime_secs: u64,
    active_runs: usize,
}

/// Get daemon status
async fn daemon_status(State(state): State<Arc<AppState>>) -> Json<DaemonStatus> {
    use crate::core::api_types::RunStatus;

    let runs = state.orchestrator.list_runs().await.unwrap_or_default();
    let active_runs = runs
        .iter()
        .filter(|r| {
            matches!(
                r.status,
                RunStatus::Working | RunStatus::Eval | RunStatus::Draft
            )
        })
        .count();

    // Calculate uptime from PID file modification time
    let uptime_secs = super::pid_path()
        .metadata()
        .and_then(|m| m.modified())
        .map(|t| {
            std::time::SystemTime::now()
                .duration_since(t)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        })
        .unwrap_or(0);

    Json(DaemonStatus {
        running: true,
        pid: std::process::id(),
        tcp_port: super::get_daemon_port(),
        hirsel_root: hirsel_dir().display().to_string(),
        uptime_secs,
        active_runs,
    })
}

/// Stop the daemon
async fn daemon_stop() -> &'static str {
    // Spawn task to exit after response is sent
    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        tracing::info!("[Daemon] Stopping via API request");
        cleanup_pid_file(&super::pid_path());
        std::process::exit(0);
    });

    "Stopping daemon"
}

// =============================================================================
// Per-run config and worker routes for workers
// =============================================================================

use axum::{http::StatusCode, response::IntoResponse};
use serde::Deserialize;

/// Helper to get SQLite state for a run
async fn get_run_state(
    state: &AppState,
    run_name: &str,
) -> Result<crate::core::state::SQLiteState, (StatusCode, String)> {
    let config = state.config.read().await;
    let run_dir = config.runs_dir().join(run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Run '{}' not found", run_name),
        ));
    }

    crate::core::state::SQLiteState::new(db_path)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

#[derive(Serialize)]
struct HitlResponse {
    enabled: bool,
}

/// Get human-in-the-loop setting for a run
async fn get_run_hitl(
    axum::extract::Path(run): axum::extract::Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    let enabled = sqlite_state.get_human_in_the_loop().unwrap_or(true);
    Json(HitlResponse { enabled }).into_response()
}

#[derive(Serialize)]
struct RequestResponse {
    request: Option<String>,
}

/// Get request (spec) for a run
async fn get_run_request(
    axum::extract::Path(run): axum::extract::Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    let request = sqlite_state.get_request().ok().flatten();
    Json(RequestResponse { request }).into_response()
}

#[derive(Serialize)]
struct ProjectPathResponse {
    project_path: Option<String>,
}

/// Get project path for a run
async fn get_run_project_path(
    axum::extract::Path(run): axum::extract::Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    let project_path = sqlite_state.get_project_path().ok().flatten();
    Json(ProjectPathResponse { project_path }).into_response()
}

#[derive(Serialize)]
struct WaitingReasonResponse {
    reason: Option<String>,
}

/// Get waiting reason for a run
async fn get_run_waiting_reason(
    axum::extract::Path(run): axum::extract::Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    let reason = sqlite_state.get_waiting_reason().ok().flatten();
    Json(WaitingReasonResponse { reason }).into_response()
}

#[derive(Deserialize)]
struct SetWaitingReasonRequest {
    reason: Option<String>,
}

/// Set waiting reason for a run
async fn set_run_waiting_reason(
    axum::extract::Path(run): axum::extract::Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<SetWaitingReasonRequest>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    match sqlite_state.set_waiting_reason(body.reason.as_deref()) {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// =============================================================================
// Worker state endpoints for remote/Docker workers
// Uses shared handlers from crate::core::server::worker_routes
// =============================================================================

use crate::core::server::worker_routes::{
    self, AllDoneResponse, ClaimedTaskResponse, HeartbeatResponse, SuccessResponse,
    UpdateWorkerRequest, WorkerResponse, WorkersResponse,
};

/// Helper to create lifecycle manager for a run
fn create_lifecycle_manager(
    run_name: &str,
    config: &crate::core::config::Config,
) -> Option<crate::core::lifecycle::LocalLifecycleManager> {
    let run_dir = config.runs_dir().join(run_name);
    let agent_command = crate::cli::config::get_agent_command();
    crate::core::lifecycle::LocalLifecycleManager::new(run_name, run_dir, agent_command).ok()
}

/// Update worker state (status, heartbeat, etc.)
async fn update_worker(
    axum::extract::Path((run, worker)): axum::extract::Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<UpdateWorkerRequest>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    // Create lifecycle manager for lifecycle event handling
    let config = state.config.read().await;
    let lifecycle = create_lifecycle_manager(&run, &config);

    match worker_routes::update_worker(
        &sqlite_state,
        &worker,
        &body,
        lifecycle
            .as_ref()
            .map(|l| l as &dyn crate::core::lifecycle::LifecycleManager),
    ) {
        Ok(_) => Json(SuccessResponse::ok()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// List all workers for the run
async fn list_workers(
    axum::extract::Path(run): axum::extract::Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    match worker_routes::list_workers(&sqlite_state) {
        Ok(workers) => Json(WorkersResponse { workers }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// List active workers for the run
async fn list_active_workers(
    axum::extract::Path(run): axum::extract::Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    match worker_routes::list_active_workers(&sqlite_state) {
        Ok(workers) => Json(WorkersResponse { workers }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Check if all workers are done
async fn all_workers_done(
    axum::extract::Path(run): axum::extract::Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    match worker_routes::all_workers_done(&sqlite_state) {
        Ok(all_done) => Json(AllDoneResponse { all_done }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Get a specific worker
async fn get_worker(
    axum::extract::Path((run, worker)): axum::extract::Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    match worker_routes::get_worker(&sqlite_state, &worker) {
        Ok(worker) => Json(WorkerResponse { worker }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Update worker heartbeat and return current run status
async fn worker_heartbeat(
    axum::extract::Path((run, worker)): axum::extract::Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    match worker_routes::worker_heartbeat(&sqlite_state, &worker) {
        Ok(status) => Json(HeartbeatResponse {
            status: status.to_string(),
        })
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Get the task claimed by a worker
async fn get_worker_claimed_task(
    axum::extract::Path((run, worker)): axum::extract::Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    match worker_routes::get_claimed_task(&sqlite_state, &worker) {
        Ok(task) => Json(ClaimedTaskResponse { task }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
