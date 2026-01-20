//! HTTP server for remote Hirsel orchestration
//!
//! This module provides a standalone HTTP server that exposes the Orchestrator
//! API for remote clients. It's used in headless server mode.

mod auth;
pub mod gyp;
pub mod routes;

use axum::{
    routing::{delete, get, patch, post},
    Router,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

use crate::core::config::Config;
use crate::core::orchestrator::LocalOrchestrator;
use crate::core::tailscale::TailscaleClient;

/// Application state shared across routes
pub struct AppState {
    pub orchestrator: LocalOrchestrator,
    /// Mutable config for API updates
    pub config: Arc<RwLock<Config>>,
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

    // Start Tailscale auth monitor if OAuth credentials are configured
    start_tailscale_monitor();

    let orchestrator = LocalOrchestrator::new(config.clone());
    let config = Arc::new(RwLock::new(config));
    let state = Arc::new(AppState {
        orchestrator,
        config,
    });
    let gyp_state = Arc::new(gyp::GypState::new());

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

    // Build the main router
    let app = Router::new()
        // Health check (no auth required)
        .route("/health", get(routes::health))
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
        // Config - read
        .route("/api/config", get(routes::get_config))
        // Config - granular updates
        .route("/api/config/general", patch(routes::patch_general_config))
        .route("/api/config/agent", patch(routes::patch_agent_config))
        .route(
            "/api/config/compaction",
            patch(routes::patch_compaction_config),
        )
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
        // Merge Gyp routes
        .merge(gyp_routes)
        // Apply auth middleware and state
        .layer(axum::middleware::from_fn_with_state(
            api_key.clone(),
            auth::api_key_auth,
        ))
        // CORS for browser-based clients
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    // Bind and serve
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("Hirsel server listening on 0.0.0.0:{}", port);

    axum::serve(listener, app).await?;

    Ok(())
}

/// Start background task to monitor Tailscale auth expiry
///
/// Reads OAuth credentials from environment variables:
/// - TAILSCALE_CLIENT_ID
/// - TAILSCALE_CLIENT_SECRET
/// - TAILSCALE_TAG (optional)
fn start_tailscale_monitor() {
    let client_id = match std::env::var("TAILSCALE_CLIENT_ID") {
        Ok(id) if !id.is_empty() => id,
        _ => {
            tracing::debug!("TAILSCALE_CLIENT_ID not set, skipping Tailscale auth monitor");
            return;
        }
    };

    let client_secret = match std::env::var("TAILSCALE_CLIENT_SECRET") {
        Ok(secret) if !secret.is_empty() => secret,
        _ => {
            tracing::warn!(
                "TAILSCALE_CLIENT_ID is set but TAILSCALE_CLIENT_SECRET is missing, \
                 skipping Tailscale auth monitor"
            );
            return;
        }
    };

    let tag = std::env::var("TAILSCALE_TAG")
        .ok()
        .filter(|t| !t.is_empty());

    tracing::info!("Starting Tailscale auth monitor (will renew 24h before expiry)");

    let client = TailscaleClient::new(client_id, client_secret, tag);

    // Spawn background task
    tokio::spawn(async move {
        // Check interval: every hour
        let check_interval = Duration::from_secs(3600);
        // Renew threshold: 24 hours before expiry
        let renew_before = Duration::from_secs(24 * 3600);

        // Initial check on startup
        match client.check_and_renew_if_needed(renew_before).await {
            Ok(renewed) => {
                if renewed {
                    tracing::info!("Tailscale auth renewed on startup");
                } else {
                    tracing::info!("Tailscale auth is valid");
                }
            }
            Err(e) => {
                tracing::error!("Failed to check Tailscale auth on startup: {}", e);
            }
        }

        // Periodic checks
        loop {
            tokio::time::sleep(check_interval).await;

            match client.check_and_renew_if_needed(renew_before).await {
                Ok(renewed) => {
                    if renewed {
                        tracing::info!("Tailscale auth renewed");
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to check/renew Tailscale auth: {}", e);
                }
            }
        }
    });
}
