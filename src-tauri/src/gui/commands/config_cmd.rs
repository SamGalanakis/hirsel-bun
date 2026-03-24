//! Configuration-related commands
//!
//! Commands for reading and writing application configuration.

use super::types::ConfigUpdateRequest;
use super::ResultExt;
use crate::core::api_types::ConfigResponse;
use crate::core::config;
use crate::core::orchestrator::{LocalOrchestrator, Orchestrator, RemoteOrchestrator};
use tauri::Manager;

/// Get application configuration stored on this client.
#[tracing::instrument]
#[tauri::command]
pub async fn get_config() -> Result<ConfigResponse, String> {
    let (config, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));
    LocalOrchestrator::new(config).get_config().await.str_err()
}

/// Save application configuration
#[tracing::instrument(skip(updates))]
#[tauri::command]
pub async fn save_config(updates: ConfigUpdateRequest) -> Result<(), String> {
    // Load existing config or create default
    let (mut cfg, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));

    // Apply LLM updates
    if let Some(llm_update) = updates.llm {
        llm_update.apply(&mut cfg.llm);
    }

    // Apply backend connection update
    if let Some(backend) = updates.backend {
        cfg.backend = backend.into();
    }

    // Apply MCP server imports
    if let Some(mcp_servers) = updates.mcp_servers {
        cfg.mcp_servers = mcp_servers;
    }

    cfg.save().context("Failed to save config")?;

    Ok(())
}

/// Check connectivity to a configured Hirsel backend.
#[tracing::instrument(skip(api_key))]
#[tauri::command]
pub async fn check_backend_health(url: String, api_key: Option<String>) -> Result<(), String> {
    let orchestrator = RemoteOrchestrator::new(url, api_key.unwrap_or_default());
    orchestrator.health().await.str_err()?;
    Ok(())
}

/// Navigate the main desktop window directly to the configured backend UI.
#[tracing::instrument(skip(app))]
#[tauri::command]
pub async fn open_backend_window(app: tauri::AppHandle) -> Result<(), String> {
    let (config, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));
    let backend = config.backend.clone();
    let base_url = backend
        .url
        .clone()
        .filter(|value: &String| !value.trim().is_empty())
        .ok_or_else(|| "Backend URL is not configured".to_string())?;
    let api_key = backend
        .api_key
        .clone()
        .filter(|value: &String| !value.trim().is_empty())
        .ok_or_else(|| "Backend API key is not configured".to_string())?;

    let orchestrator = RemoteOrchestrator::new(base_url.clone(), api_key.clone());
    orchestrator.health().await.str_err()?;

    let mut url = base_url.trim_end_matches('/').to_string();
    url.push_str("/connect/bootstrap");
    let mut url = url
        .parse::<tauri::Url>()
        .map_err(|error| format!("Invalid backend URL: {}", error))?;
    url.query_pairs_mut()
        .append_pair("api_key", &api_key)
        .append_pair("return_to", "/app");

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;
    window
        .navigate(url)
        .map_err(|error| format!("Failed to navigate to backend: {}", error))?;
    Ok(())
}
