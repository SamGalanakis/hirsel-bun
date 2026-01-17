//! HTTP server for remote Hirsel orchestration
//!
//! This module provides a standalone HTTP server that exposes the Orchestrator
//! API for remote clients. It's used in headless server mode.

mod auth;
pub mod gyp;
mod routes;

use axum::{
    routing::{delete, get, post},
    Router,
};
use std::sync::Arc;
use tokio::net::TcpListener;

use crate::core::config::Config;
use crate::core::orchestrator::LocalOrchestrator;

/// Application state shared across routes
pub struct AppState {
    pub orchestrator: LocalOrchestrator,
}

/// Start the HTTP server
pub async fn start_server(port: u16) -> anyhow::Result<()> {
    // Load API key from environment
    let api_key = std::env::var("HIRSEL_API_KEY")
        .map_err(|_| anyhow::anyhow!("HIRSEL_API_KEY environment variable is required"))?;

    // Load config and create local orchestrator
    let (config, warnings) =
        Config::load().map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))?;
    for warning in warnings {
        tracing::warn!("{}", warning);
    }

    let orchestrator = LocalOrchestrator::new(config);
    let state = Arc::new(AppState { orchestrator });
    let gyp_state = Arc::new(gyp::GypState::new());

    // Build Gyp chat routes with separate state
    let gyp_routes = Router::new()
        .route(
            "/api/gyp/sessions",
            get(gyp::list_sessions).post(gyp::start_session),
        )
        .route("/api/gyp/sessions/:id", delete(gyp::stop_session))
        .route("/api/gyp/sessions/:id/messages", post(gyp::send_message))
        .route(
            "/api/gyp/sessions/:id/permission",
            post(gyp::respond_permission),
        )
        .route("/api/gyp/sessions/:id/events", get(gyp::session_events))
        .with_state(gyp_state);

    // Build the main router
    let app = Router::new()
        // Health check (no auth required)
        .route("/health", get(routes::health))
        // Run management
        .route("/api/runs", get(routes::list_runs))
        .route(
            "/api/runs/:name",
            get(routes::get_run).delete(routes::delete_run),
        )
        .route("/api/runs/:name/pause", post(routes::pause_run))
        .route("/api/runs/:name/resume", post(routes::resume_run))
        .route("/api/runs/:name/deliver", post(routes::deliver_run))
        // Workers
        .route("/api/runs/:name/workers", get(routes::list_workers))
        .route(
            "/api/runs/:name/workers/:worker/restart",
            post(routes::restart_worker),
        )
        .route(
            "/api/runs/:name/workers/:worker/log",
            get(routes::get_worker_log),
        )
        .route(
            "/api/runs/:name/workers/:worker/events",
            get(routes::get_worker_events),
        )
        // Tasks
        .route(
            "/api/runs/:name/tasks",
            get(routes::list_tasks).post(routes::add_task),
        )
        .route(
            "/api/runs/:name/tasks/:task_id",
            delete(routes::delete_task),
        )
        .route(
            "/api/runs/:name/tasks/:task_id/complete",
            post(routes::complete_task),
        )
        .route(
            "/api/runs/:name/tasks/:task_id/reopen",
            post(routes::reopen_task),
        )
        // Threads and messages
        .route("/api/runs/:name/threads", get(routes::list_threads))
        .route(
            "/api/runs/:name/threads/:thread/messages",
            get(routes::get_messages).post(routes::send_message),
        )
        // Evals
        .route("/api/runs/:name/evals", get(routes::list_evals))
        // History
        .route("/api/runs/:name/history", get(routes::get_history))
        // Assets
        .route("/api/runs/:name/assets", post(gyp::upload_asset))
        .route("/api/runs/:name/assets-path", get(gyp::get_assets_path))
        // Config
        .route("/api/config", get(routes::get_config))
        // Merge Gyp routes
        .merge(gyp_routes)
        // Apply auth middleware and state
        .layer(axum::middleware::from_fn_with_state(
            api_key.clone(),
            auth::api_key_auth,
        ))
        .with_state(state);

    // Bind and serve
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("Hirsel server listening on 0.0.0.0:{}", port);

    axum::serve(listener, app).await?;

    Ok(())
}
