//! HTTP server for the Hirsel web app.
//!
//! The server hosts the project/thread/canvas UI and the small amount of auth
//! and settings surface the thin desktop shell needs.

mod auth;
pub mod web_routes;

use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

use crate::backend::config::Config;

/// Application state shared across HTTP handlers.
pub struct AppState {
    pub api_key: String,
    /// Mutable config for API updates
    pub config: Arc<RwLock<Config>>,
}

/// Start the HTTP server
pub async fn start_server(port: u16) -> anyhow::Result<()> {
    // Load API key from environment
    let api_key = std::env::var("HIRSEL_API_KEY")
        .map_err(|_| anyhow::anyhow!("HIRSEL_API_KEY environment variable is required"))?;

    // Load config
    let (config, warnings) =
        Config::load().map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))?;
    for warning in warnings {
        tracing::warn!("{}", warning);
    }

    let config = Arc::new(RwLock::new(config));
    let state = Arc::new(AppState {
        api_key: api_key.clone(),
        config,
    });
    crate::backend::shepherd_runtime::start_server_control_listener()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to start server control socket: {}", error))?;
    let app = web_routes::build_web_routes()
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
