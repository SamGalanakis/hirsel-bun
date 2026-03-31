//! HTTP server for the Hirsel web app.
//!
//! Serves the SolidJS SPA from webui/dist/ and JSON API routes at /api/*.

mod auth;
pub mod web_routes;

use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use crate::backend::config::Config;

/// Application state shared across HTTP handlers.
pub struct AppState {
    pub api_key: String,
    /// Mutable config for API updates
    pub config: Arc<RwLock<Config>>,
}

fn resolve_webui_dist() -> PathBuf {
    // Check HIRSEL_WEBUI_DIR env var first, then common locations
    if let Ok(dir) = std::env::var("HIRSEL_WEBUI_DIR") {
        return PathBuf::from(dir);
    }
    let candidates = [PathBuf::from("webui/dist"), PathBuf::from("../webui/dist")];
    for candidate in &candidates {
        if candidate.join("index.html").exists() {
            return candidate.clone();
        }
    }
    // Fallback — will 404 gracefully if not found
    PathBuf::from("webui/dist")
}

/// Start the HTTP server
pub async fn start_server(port: u16) -> anyhow::Result<()> {
    let api_key = std::env::var("HIRSEL_API_KEY").unwrap_or_default();

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

    // Resolve SPA directory
    let webui_dir = resolve_webui_dist();
    let index_html = webui_dir.join("index.html");
    tracing::info!(path = %webui_dir.display(), "Serving SPA from");

    // Build router: API routes first, then SPA fallback
    let app = web_routes::build_web_routes()
        .with_state(state)
        .layer(axum::middleware::from_fn_with_state(
            api_key.clone(),
            auth::api_key_auth,
        ))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        // Serve SPA static files, falling back to index.html for client-side routing
        .fallback_service(ServeDir::new(&webui_dir).fallback(ServeFile::new(&index_html)));

    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("Hirsel server listening on 0.0.0.0:{}", port);
    if auth::auth_enabled(&api_key) {
        tracing::info!("HTTP API key auth enabled");
    } else {
        tracing::info!("HTTP API key auth disabled");
    }

    axum::serve(listener, app).await?;

    Ok(())
}
