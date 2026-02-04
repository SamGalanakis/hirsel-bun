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
//! | `build_gyp_routes()` | Both | Gyp chat sessions (separate state) |
//! | `build_config_routes()` | Remote only | Config CRUD, credentials |
//! | `build_board_routes()` | Remote only | Board sync for SpecFlow |

use axum::{
    routing::{delete, get, patch, post},
    Router,
};
use std::sync::Arc;

use super::{board, gyp, routes, AppState};

/// Routes shared between daemon and remote server
///
/// These handle core run operations that both servers need.
pub fn build_shared_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Health check
        .route("/health", get(routes::health))
        // Run management
        .route("/api/runs", get(routes::list_runs).post(routes::create_run))
        .route("/api/runs/start", post(routes::start_run))
        .route(
            "/api/runs/{name}",
            get(routes::get_run).delete(routes::delete_run),
        )
        .route(
            "/api/runs/{name}/files",
            get(routes::download_files).post(routes::upload_files),
        )
        .route("/api/runs/{name}/workspace", post(routes::init_workspace))
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
        // Threads and messages (run-level - used by orchestrator/GUI)
        .route("/api/runs/{name}/threads", get(routes::list_threads))
        .route(
            "/api/runs/{name}/threads/{thread}/messages",
            get(routes::get_messages).post(routes::send_message),
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
        // Assets
        .route("/api/runs/{name}/assets", post(gyp::upload_asset))
        .route("/api/runs/{name}/assets-path", get(gyp::get_assets_path))
        // Config - read only (both servers can read)
        .route("/api/config", get(routes::get_config))
        // Board integration - for workers in board runs
        .route(
            "/api/runs/{name}/config/project_id",
            get(routes::get_project_id),
        )
        // Live nodes - unified task system
        .route(
            "/api/runs/{name}/live-nodes",
            get(routes::get_live_nodes).post(routes::add_live_node),
        )
        .route(
            "/api/runs/{name}/live-nodes/claimable",
            get(routes::get_claimable_live_nodes),
        )
        .route(
            "/api/runs/{name}/live-nodes/{id}/claim",
            post(routes::claim_live_node),
        )
        .route(
            "/api/runs/{name}/live-nodes/{id}/complete",
            post(routes::complete_live_node),
        )
        .route(
            "/api/runs/{name}/live-nodes/{id}/unclaim",
            post(routes::unclaim_live_node),
        )
        .route(
            "/api/runs/{name}/live-nodes/{id}/blocked",
            get(routes::is_live_node_blocked),
        )
        .route(
            "/api/runs/{name}/live-nodes/{id}/eval-pass",
            post(routes::live_node_eval_pass),
        )
        .route(
            "/api/runs/{name}/live-nodes/{id}/eval-fail",
            post(routes::live_node_eval_fail),
        )
        .route(
            "/api/runs/{name}/live-nodes/{id}/tokens",
            post(routes::set_live_node_tokens),
        )
        .route(
            "/api/runs/{name}/live-nodes/{id}/validated",
            get(routes::get_validated_nodes),
        )
}

/// Gyp chat routes (requires separate GypState)
///
/// These handle the Gyp AI assistant chat functionality.
pub fn build_gyp_routes() -> Router<Arc<gyp::GypState>> {
    Router::new()
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
}

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
        .route("/api/config/auth", get(routes::get_auth_config))
        .route(
            "/api/config/auth/{agent}",
            patch(routes::patch_agent_auth).delete(routes::delete_agent_auth),
        )
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
}

/// Board sync routes (remote server only)
///
/// These handle SpecFlow board synchronization.
pub fn build_board_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/board/{project_id}/export", post(board::export_board))
        .route("/api/board/{project_id}/import", post(board::import_board))
        .route(
            "/api/board/{project_id}/directory",
            get(board::get_board_directory),
        )
        // Per-task file routes
        .route("/api/board/{project_id}/tasks", get(board::list_task_files))
        .route(
            "/api/board/{project_id}/tasks/{slug}",
            get(board::get_task_file)
                .post(board::write_task_file)
                .delete(board::delete_task_file),
        )
}
