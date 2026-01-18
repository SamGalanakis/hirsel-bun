//! Unix socket server for the hirsel daemon
//!
//! Reuses the existing axum router from core/server/ but binds to a Unix socket
//! instead of a TCP socket.

use anyhow::Result;
use axum::{
    routing::{delete, get, post},
    Router,
};
use std::path::Path;
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

use crate::core::config::Config;
use crate::core::orchestrator::{LocalOrchestrator, Orchestrator};
use crate::core::server::{gyp, AppState};

use super::lifecycle;

/// Daemon configuration
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Idle timeout in seconds (daemon exits if no active runs for this long)
    /// Set to 0 to disable auto-exit
    pub idle_timeout_secs: u64,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            idle_timeout_secs: 300, // 5 minutes
        }
    }
}

/// Start the daemon server on a Unix socket
pub async fn start_daemon(config: DaemonConfig) -> Result<()> {
    let socket_path = super::socket_path();
    let pid_path = super::pid_path();

    // Ensure the parent directory exists
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Remove stale socket if it exists
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }

    // Write PID file
    let pid = std::process::id();
    std::fs::write(&pid_path, pid.to_string())?;

    // Create cleanup handler for graceful shutdown
    let socket_path_clone = socket_path.clone();
    let pid_path_clone = pid_path.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("[Daemon] Received shutdown signal");
        cleanup_socket(&socket_path_clone, &pid_path_clone);
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

    // Build router (same routes as core/server but no auth for local socket)
    let gyp_state = Arc::new(gyp::GypState::new());
    let router = build_router(state, gyp_state);

    // Bind to Unix socket
    let listener = UnixListener::bind(&socket_path)?;
    tracing::info!(
        "[Daemon] Listening on Unix socket: {}",
        socket_path.display()
    );
    tracing::info!("[Daemon] PID: {}", pid);

    // Serve requests
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let router = router.clone();
                tokio::spawn(async move {
                    let io = hyper_util::rt::TokioIo::new(stream);
                    let service =
                        hyper_util::service::TowerToHyperService::new(router.into_service());
                    if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(io, service)
                    .await
                    {
                        tracing::warn!("[Daemon] Connection error: {}", e);
                    }
                });
            }
            Err(e) => {
                tracing::warn!("[Daemon] Accept error: {}", e);
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

    // Build the main router (no auth layer for Unix socket - filesystem permissions are enough)
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
            "/api/runs/{name}/workers/{worker}/log",
            get(routes::get_worker_log),
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

/// Clean up socket and PID file
pub(crate) fn cleanup_socket(socket_path: &Path, pid_path: &Path) {
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }
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
        cleanup_socket(&super::socket_path(), &super::pid_path());
        std::process::exit(0);
    });

    "Stopping daemon"
}
