//! Configuration-related commands
//!
//! Commands for reading and writing application configuration.

use super::types::ConfigUpdateRequest;
use super::ResultExt;
use crate::backend::api_types::ConfigResponse;
use crate::backend::config;
use tauri::Manager;

async fn backend_health(url: &str, api_key: Option<&str>) -> Result<(), String> {
    let client = reqwest::Client::new();
    let mut request = client.get(format!("{}/health", url.trim_end_matches('/')));
    if let Some(api_key) = api_key.filter(|value| !value.trim().is_empty()) {
        request = request.bearer_auth(api_key);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Backend request failed: {}", e))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "Backend health check returned HTTP {}",
            response.status()
        ))
    }
}

/// Get application configuration stored on this client.
#[tracing::instrument]
#[tauri::command]
pub async fn get_config() -> Result<ConfigResponse, String> {
    let (config, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));
    Ok(ConfigResponse {
        llm: config.llm.into(),
        backend: config.backend.into(),
        mcp_servers: config.mcp_servers,
    })
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
    backend_health(&url, api_key.as_deref()).await
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

    backend_health(&base_url, Some(&api_key)).await?;

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
