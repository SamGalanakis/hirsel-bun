//! Shared route builders for the daemon and backend HTTP server
//!
//! Routes are defined in builder functions to avoid duplication between:
//! - `daemon/server.rs` (local daemon, no auth)
//! - `backend/server/mod.rs` (backend server, with auth)
//!
//! ## Route Categories
//!
//! | Builder | Used By | Description |
//! |---------|---------|-------------|
//! | `build_shared_routes()` | Both | Runtime ops, workers, tasks, evals, history |
//! | `build_readonly_config_routes()` | Daemon | Read-only config endpoint |
//! | `build_config_routes()` | Backend only | Config CRUD, credentials |

use axum::{
    routing::{get, patch, post},
    Router,
};
use std::sync::Arc;

use super::{routes, AppState};

pub fn build_web_routes() -> Router<Arc<AppState>> {
    super::web_routes::build_web_routes()
}

/// Routes shared between daemon and remote server
///
/// These handle core route-runtime operations that both servers need.
pub fn build_shared_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Health check
        .route("/health", get(routes::health))
        // Runtime management
        .route("/api/runtimes", get(routes::list_runs))
        .route("/api/runtimes/start", post(routes::start_run))
        .route(
            "/api/runtimes/{name}",
            get(routes::get_run).delete(routes::delete_run),
        )
        .route("/api/runtimes/{name}/files", get(routes::download_files))
        .route(
            "/api/runtimes/{name}/workspace",
            post(routes::init_workspace),
        )
        .route("/api/runtimes/{name}/pause", post(routes::pause_run))
        .route("/api/runtimes/{name}/resume", post(routes::resume_run))
        .route("/api/runtimes/{name}/deliver", post(routes::deliver_run))
        // Workers
        .route("/api/runtimes/{name}/workers", get(routes::list_workers))
        .route(
            "/api/runtimes/{name}/workers/{worker}/restart",
            post(routes::restart_worker),
        )
        .route(
            "/api/runtimes/{name}/workers/{worker}/spawn",
            post(routes::spawn_single_worker),
        )
        .route(
            "/api/runtimes/{name}/workers/{worker}/resume",
            post(routes::resume_worker),
        )
        .route(
            "/api/runtimes/{name}/workers/{worker}/events",
            get(routes::get_worker_events),
        )
        .route("/api/runtimes/{name}/scribe", post(routes::add_scribe))
        .route(
            "/api/runtimes/{name}/retained-context",
            get(routes::get_retained_context),
        )
        // Evals
        .route("/api/runtimes/{name}/evals", get(routes::list_evals))
        // History
        .route("/api/runtimes/{name}/history", get(routes::get_history))
        // Board integration - for workers in route runtimes
        .route(
            "/api/runtimes/{name}/config/project_id",
            get(routes::get_project_id),
        )
        // Nodes - unified task system
        .route(
            "/api/runtimes/{name}/nodes",
            get(routes::get_nodes).post(routes::add_node),
        )
        .route(
            "/api/runtimes/{name}/nodes/claimable",
            get(routes::get_claimable_nodes),
        )
        .route(
            "/api/runtimes/{name}/nodes/{id}/claim",
            post(routes::claim_node),
        )
        .route(
            "/api/runtimes/{name}/nodes/{id}/complete",
            post(routes::complete_node),
        )
        .route(
            "/api/runtimes/{name}/nodes/{id}/unclaim",
            post(routes::unclaim_node),
        )
        .route(
            "/api/runtimes/{name}/nodes/{id}/blocked",
            get(routes::is_node_blocked),
        )
        .route(
            "/api/runtimes/{name}/nodes/{id}/check-pass",
            post(routes::node_check_pass),
        )
        .route(
            "/api/runtimes/{name}/nodes/{id}/check-fail",
            post(routes::node_check_fail),
        )
        .route(
            "/api/runtimes/{name}/nodes/{id}/tokens",
            post(routes::set_node_tokens),
        )
        .route(
            "/api/runtimes/{name}/nodes/{id}/validated",
            get(routes::get_validated_nodes),
        )
}

// Shepherd routes were removed during lash migration.
/// Read-only config routes for the local daemon.
pub fn build_readonly_config_routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/config", get(routes::get_config))
}

/// Config management routes (backend server only)
///
/// Full config CRUD for the remote server.
pub fn build_config_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Config - full CRUD (includes GET, so do not merge with read-only config routes)
        .route(
            "/api/config",
            get(routes::get_config)
                .put(routes::put_config)
                .patch(routes::patch_config),
        )
        .route("/api/config/llm", patch(routes::patch_llm_config))
        // Credentials
        .route(
            "/api/credentials/{key}",
            post(routes::store_credential)
                .get(routes::get_credential)
                .delete(routes::delete_credential),
        )
        // Codex OAuth device flow
        .route(
            "/api/auth/codex/device/start",
            post(routes::codex_device_start),
        )
        .route(
            "/api/auth/codex/device/poll",
            post(routes::codex_device_poll),
        )
        .route(
            "/api/auth/codex/device/exchange",
            post(routes::codex_device_exchange),
        )
}
