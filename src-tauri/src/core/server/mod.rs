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
pub mod board;
pub mod eval_routes;
pub mod routes;
pub mod shared_routes;
pub mod worker_routes;

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
    // Build the router using shared route builders
    // Remote server gets: shared routes + config routes + board routes
    let app = shared_routes::build_shared_routes()
        .merge(shared_routes::build_config_routes())
        .merge(shared_routes::build_board_routes())
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
