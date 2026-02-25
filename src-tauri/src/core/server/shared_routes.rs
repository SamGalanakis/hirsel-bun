//! Shared route builders for daemon and remote server
//!
//! Routes are defined in builder functions to avoid duplication between:
//! - `daemon/server.rs` (local daemon, no auth)
//! - `core/server/mod.rs` (remote server, with auth)
//!
//! ## Route Categories
//!
//! | Builder | Used By | Description |
//! |---------|---------|-------------|
//! | `build_shared_routes()` | Both | Run ops, workers, tasks, messages, evals, history |
//! | `build_legacy_chat_routes()` | N/A | Removed during lash migration |
//! | `build_config_routes()` | Remote only | Config CRUD, credentials |
//! | `build_board_routes()` | Remote only | Reserved (board file sync removed) |

use axum::{
    routing::{any, get, patch, post},
    Router,
};
use std::sync::Arc;

use super::{routes, AppState};
use crate::core::git_http;

/// Routes shared between daemon and remote server
///
/// These handle core run operations that both servers need.
pub fn build_shared_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Health check
        .route("/health", get(routes::health))
        // Run management
        .route("/api/runs", get(routes::list_runs))
        .route("/api/runs/start", post(routes::start_run))
        .route(
            "/api/runs/{name}",
            get(routes::get_run).delete(routes::delete_run),
        )
        .route("/api/runs/{name}/files", get(routes::download_files))
        .route("/api/runs/{name}/workspace", post(routes::init_workspace))
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
        // Project messages (Sheepfold - used by workers via StateAccess)
        .route(
            "/api/projects/{project_id}/messages",
            post(routes::add_project_message),
        )
        .route(
            "/api/projects/{project_id}/messages/{thread}",
            get(routes::get_project_messages),
        )
        .route(
            "/api/projects/{project_id}/messages/{thread}/unread/{reader}",
            get(routes::get_unread_project_messages),
        )
        .route(
            "/api/projects/{project_id}/messages/{thread}/mark-read",
            post(routes::mark_project_messages_read),
        )
        .route(
            "/api/projects/{project_id}/messages/unread/{reader}",
            get(routes::get_all_unread_project_messages),
        )
        .route(
            "/api/projects/{project_id}/threads",
            get(routes::get_project_threads),
        )
        // Scribe - documentation
        .route("/api/runs/{name}/scribe", post(routes::add_scribe))
        .route("/api/runs/{name}/docs", get(routes::get_docs))
        .route("/api/runs/{name}/docs/sync", post(routes::sync_docs))
        // Evals
        .route("/api/runs/{name}/evals", get(routes::list_evals))
        // History
        .route("/api/runs/{name}/history", get(routes::get_history))
        // Config - read only (both servers can read)
        .route("/api/config", get(routes::get_config))
        // Board integration - for workers in board runs
        .route(
            "/api/runs/{name}/config/project_id",
            get(routes::get_project_id),
        )
        // Nodes - unified task system
        .route(
            "/api/runs/{name}/nodes",
            get(routes::get_nodes).post(routes::add_node),
        )
        .route(
            "/api/runs/{name}/nodes/claimable",
            get(routes::get_claimable_nodes),
        )
        .route(
            "/api/runs/{name}/nodes/{id}/claim",
            post(routes::claim_node),
        )
        .route(
            "/api/runs/{name}/nodes/{id}/complete",
            post(routes::complete_node),
        )
        .route(
            "/api/runs/{name}/nodes/{id}/unclaim",
            post(routes::unclaim_node),
        )
        .route(
            "/api/runs/{name}/nodes/{id}/blocked",
            get(routes::is_node_blocked),
        )
        .route(
            "/api/runs/{name}/nodes/{id}/check-pass",
            post(routes::node_check_pass),
        )
        .route(
            "/api/runs/{name}/nodes/{id}/check-fail",
            post(routes::node_check_fail),
        )
        .route(
            "/api/runs/{name}/nodes/{id}/tokens",
            post(routes::set_node_tokens),
        )
        .route(
            "/api/runs/{name}/nodes/{id}/validated",
            get(routes::get_validated_nodes),
        )
        // Git HTTP backend for remote workers
        .route("/git/{run_name}", any(git_http::git_run_root_handler))
        .route("/git/{run_name}/{*path}", any(git_http::git_run_handler))
}

/// Shepherd routes were removed during lash migration.

/// Config management routes (remote server only)
///
/// Full config CRUD - daemon only exposes read-only config endpoint.
pub fn build_config_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Config - full CRUD (overwrites the read-only route from shared)
        .route(
            "/api/config",
            get(routes::get_config)
                .put(routes::put_config)
                .patch(routes::patch_config),
        )
        // Granular config updates
        .route("/api/config/general", patch(routes::patch_general_config))
        .route("/api/config/agent", patch(routes::patch_agent_config))
        .route("/api/config/llm", patch(routes::patch_llm_config))
        .route("/api/config/runners", get(routes::list_runners))
        .route(
            "/api/config/runners/{name}",
            get(routes::get_runner)
                .put(routes::put_runner)
                .delete(routes::delete_runner),
        )
        .route("/api/config/profiles", get(routes::list_profiles))
        .route(
            "/api/config/profiles/{name}",
            get(routes::get_profile)
                .put(routes::put_profile)
                .delete(routes::delete_profile),
        )
        .route("/api/config/git", patch(routes::patch_git_config))
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

/// Board file-sync routes removed (DB-only board editing).
pub fn build_board_routes() -> Router<Arc<AppState>> {
    Router::new()
}
