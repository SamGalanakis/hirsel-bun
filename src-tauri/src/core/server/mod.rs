//! HTTP server for remote Hirsel orchestration
//!
//! This module provides a standalone HTTP server that exposes the Orchestrator
//! API for remote clients. It's used in headless server mode.
//!
//! Route handlers for state operations are in submodules:
//! - `eval_routes` - Eval API endpoints
//! - `worker_routes` - Worker API endpoints
//! - `shared_routes` - Route builders shared with daemon

mod auth;
pub mod eval_routes;
pub mod routes;
pub mod shared_routes;
pub mod worker_routes;

use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

use crate::core::config::Config;
use crate::core::orchestrator::LocalOrchestrator;

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

    let orchestrator = LocalOrchestrator::new(config.clone());
    let config = Arc::new(RwLock::new(config));
    let state = Arc::new(AppState {
        orchestrator,
        config,
    });
    // Build the router using shared route builders.
    // Remote server gets the shared runtime routes plus backend config/auth routes.
    let app = shared_routes::build_shared_routes()
        .merge(shared_routes::build_config_routes())
        .with_state(state)
        // Apply auth middleware
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
        );

    // Bind and serve
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("Hirsel server listening on 0.0.0.0:{}", port);

    axum::serve(listener, app).await?;

    Ok(())
}
