//! Configuration-related commands
//!
//! Commands for reading and writing application configuration.

use super::types::ConfigUpdateRequest;
use super::ResultExt;
use crate::backend::api_types::ConfigResponse;
use crate::backend::config;
use tauri::Manager;

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
        .filter(|value| !value.trim().is_empty());

    let target = if let Some(api_key) = api_key {
        let mut url = format!("{}/connect/bootstrap", base_url.trim_end_matches('/'))
            .parse::<tauri::Url>()
            .map_err(|error| format!("Invalid backend URL: {}", error))?;
        url.query_pairs_mut()
            .append_pair("api_key", &api_key)
            .append_pair("return_to", "/app");
        url
    } else {
        format!("{}/app", base_url.trim_end_matches('/'))
            .parse::<tauri::Url>()
            .map_err(|error| format!("Invalid backend URL: {}", error))?
    };

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;
    window
        .navigate(target)
        .map_err(|error| format!("Failed to navigate to backend: {}", error))?;
    Ok(())
}
