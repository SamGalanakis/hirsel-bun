//! HTTP server for the Hirsel web app.
//!
//! Serves the SolidJS SPA from webui/dist/ and JSON API routes at /api/*.

mod auth;
pub mod web_routes;

use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use crate::backend::config::Config;

/// Application state shared across HTTP handlers.
pub struct AppState {
    pub api_key: String,
}

fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn resolve_http_api_key() -> String {
    let api_key = std::env::var("HIRSEL_API_KEY").unwrap_or_default();

    #[cfg(debug_assertions)]
    {
        if !api_key.trim().is_empty() && !env_flag_enabled("HIRSEL_DEV_AUTH") {
            tracing::info!(
                "Ignoring HIRSEL_API_KEY in debug build; set HIRSEL_DEV_AUTH=1 to enable HTTP auth"
            );
            return String::new();
        }
    }

    api_key
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
    let api_key = resolve_http_api_key();

    let (_config, warnings) =
        Config::load().map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))?;
    for warning in warnings {
        tracing::warn!("{}", warning);
    }

    let state = Arc::new(AppState {
        api_key: api_key.clone(),
    });

    crate::backend::shepherd_runtime::scrub_stale_startup_state()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to scrub stale shepherd state: {}", error))?;

    // Resolve SPA directory
    let webui_dir = resolve_webui_dist();
    let index_html = webui_dir.join("index.html");
    let assets_dir = webui_dir.join("assets");
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
        .nest_service("/assets", ServeDir::new(&assets_dir))
        .route_service(
            "/favicon.ico",
            ServeFile::new(webui_dir.join("favicon.ico")),
        )
        .fallback({
            let index_html = index_html.clone();
            move || {
                let index_html = index_html.clone();
                async move {
                    match tokio::fs::read(&index_html).await {
                        Ok(bytes) => axum::http::Response::builder()
                            .status(axum::http::StatusCode::OK)
                            .header(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
                            .header(
                                axum::http::header::CACHE_CONTROL,
                                "no-store, no-cache, must-revalidate",
                            )
                            .body(axum::body::Body::from(bytes))
                            .expect("failed to build index response"),
                        Err(_) => axum::http::Response::builder()
                            .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                            .body(axum::body::Body::empty())
                            .expect("failed to build 500 response"),
                    }
                }
            }
        });

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
