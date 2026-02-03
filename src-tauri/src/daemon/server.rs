//! TCP server for the hirsel daemon
//!
//! Listens on a TCP port (default 19700, configurable via HIRSEL_DAEMON_PORT) for HTTP requests.
//! Local CLI/GUI connects via localhost, remote workers via Docker host or SSH tunnels.

use anyhow::{anyhow, Result};
use axum::{
    routing::{get, post},
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

    // Write PID file with binary path and mtime for mismatch detection
    let pid = std::process::id();
    let exe_path = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    // Get mtime of the binary for identity checking (catches same-path rebuilds)
    let mtime = std::fs::metadata(&exe_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Format: pid\npath\nmtime
    std::fs::write(&pid_path, format!("{}\n{}\n{}", pid, exe_path, mtime))?;

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
    use crate::core::server::shared_routes;

    // Build the router using shared route builders
    // Daemon gets: shared routes + daemon-specific routes + gyp routes
    // No auth layer for local daemon - localhost only
    shared_routes::build_shared_routes()
        // Daemon-specific routes
        .route("/daemon/health", get(daemon_health))
        .route("/daemon/stop", post(daemon_stop))
        .route("/daemon/status", get(daemon_status))
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
        .route("/api/runs/{run}/scaling_check", post(request_scaling_check))
        .route("/api/runs/{run}/workers/{worker}", get(get_worker))
        .route(
            "/api/runs/{run}/workers/{worker}/update",
            post(update_worker),
        )
        .route(
            "/api/runs/{run}/workers/{worker}/heartbeat",
            post(worker_heartbeat),
        )
        .with_state(state)
        // Merge Gyp routes (with separate state)
        .merge(shared_routes::build_gyp_routes().with_state(gyp_state))
        // CORS for browser-based clients
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
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
struct DaemonHealth {
    ok: bool,
    version: String,
    git_sha: String,
}

#[derive(Serialize)]
struct DaemonStatus {
    running: bool,
    pid: u32,
    tcp_port: u16,
    hirsel_root: String,
    runs_dir: String,
    uptime_secs: u64,
    active_runs: usize,
    version: String,
    git_sha: String,
    build_date: String,
}

/// Simple health check for quick daemon alive detection
async fn daemon_health() -> Json<DaemonHealth> {
    use crate::version;
    Json(DaemonHealth {
        ok: true,
        version: version::VERSION.to_string(),
        git_sha: version::GIT_SHA.to_string(),
    })
}

/// Get daemon status
async fn daemon_status(State(state): State<Arc<AppState>>) -> Json<DaemonStatus> {
    use crate::core::api_types::RunStatus;
    use crate::version;

    let runs = match state.orchestrator.list_runs().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(
                "[Daemon] Failed to list runs for status: {} - returning empty list",
                e
            );
            Vec::new()
        }
    };
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

    // Get runs_dir from config for debugging path issues
    let runs_dir = state.config.read().await.runs_dir().display().to_string();

    Json(DaemonStatus {
        running: true,
        pid: std::process::id(),
        tcp_port: super::get_daemon_port(),
        hirsel_root: hirsel_dir().display().to_string(),
        runs_dir,
        uptime_secs,
        active_runs,
        version: version::VERSION.to_string(),
        git_sha: version::GIT_SHA.to_string(),
        build_date: version::BUILD_DATE.to_string(),
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

    crate::core::state::SQLiteState::new(run_name)
        .await
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

    let enabled = sqlite_state.get_human_in_the_loop().await.unwrap_or(true);
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

    let request = sqlite_state.get_request().await.ok().flatten();
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

    let project_path = sqlite_state.get_project_path().await.ok().flatten();
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

    let reason = sqlite_state.get_waiting_reason().await.ok().flatten();
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

    match sqlite_state
        .set_waiting_reason(body.reason.as_deref())
        .await
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// =============================================================================
// Worker state endpoints for remote/Docker workers
// Uses shared handlers from crate::core::server::worker_routes
// =============================================================================

use crate::core::server::worker_routes::{
    self, AllDoneResponse, HeartbeatResponse, SuccessResponse, UpdateWorkerRequest, WorkerResponse,
    WorkersResponse,
};

/// Helper to create lifecycle manager for a run
async fn create_lifecycle_manager(
    run_name: &str,
    config: &crate::core::config::Config,
) -> Option<crate::core::lifecycle::LocalLifecycleManager> {
    let run_dir = config.runs_dir().join(run_name);
    let agent_command = crate::cli::config::get_agent_command();
    match crate::core::lifecycle::LocalLifecycleManager::new(run_name, run_dir, agent_command).await
    {
        Ok(lm) => Some(lm),
        Err(e) => {
            tracing::warn!(
                "[Daemon] Failed to create lifecycle manager for '{}': {} - lifecycle events disabled",
                run_name,
                e
            );
            None
        }
    }
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
    let lifecycle = create_lifecycle_manager(&run, &config).await;

    match worker_routes::update_worker(&sqlite_state, &worker, &body, lifecycle.as_ref()).await {
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

    match worker_routes::list_workers(&sqlite_state).await {
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

    match worker_routes::list_active_workers(&sqlite_state).await {
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

    match worker_routes::all_workers_done(&sqlite_state).await {
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

    match worker_routes::get_worker(&sqlite_state, &worker).await {
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

    match worker_routes::worker_heartbeat(&sqlite_state, &worker).await {
        Ok(status) => Json(HeartbeatResponse {
            status: status.to_string(),
        })
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Request a scaling check (triggers event-driven worker spawn/assignment)
async fn request_scaling_check(
    axum::extract::Path(run): axum::extract::Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let sqlite_state = match get_run_state(&state, &run).await {
        Ok(s) => s,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    match worker_routes::request_scaling_check(&sqlite_state).await {
        Ok(_) => Json(SuccessResponse::ok()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
