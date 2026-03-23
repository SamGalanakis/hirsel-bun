//! Configuration-related commands
//!
//! Commands for reading and writing application configuration.

use super::types::ConfigUpdateRequest;
use super::ResultExt;
use crate::core::api_types::ConfigResponse;
use crate::core::config;
use crate::core::credentials::{CodexOAuthCredentials, CredentialStore};
use crate::core::orchestrator::{LocalOrchestrator, Orchestrator, RemoteOrchestrator};
use lash::oauth;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDeviceStartResponse {
    pub device_auth_id: String,
    pub user_code: String,
    pub verify_url: String,
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDevicePollResponse {
    pub status: String,
    pub authorization_code: Option<String>,
    pub code_verifier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDeviceExchangeResponse {
    pub status: String,
    pub expires_at: u64,
}

/// Start Codex device-code OAuth flow for GUI.
#[tracing::instrument]
#[tauri::command]
pub async fn codex_device_start_gui() -> Result<CodexDeviceStartResponse, String> {
    let device = oauth::codex_request_device_code()
        .await
        .map_err(|e| format!("Failed to start Codex device auth: {}", e))?;

    Ok(CodexDeviceStartResponse {
        device_auth_id: device.device_auth_id,
        user_code: device.user_code,
        verify_url: oauth::CODEX_DEVICE_VERIFY_URL.to_string(),
        interval: device.interval,
    })
}

/// Poll Codex device authorization status for GUI.
#[tracing::instrument(skip(user_code))]
#[tauri::command]
pub async fn codex_device_poll_gui(
    device_auth_id: String,
    user_code: String,
) -> Result<CodexDevicePollResponse, String> {
    let polled = oauth::codex_poll_device_auth(&device_auth_id, &user_code)
        .await
        .map_err(|e| format!("Failed to poll Codex device auth: {}", e))?;

    match polled {
        Some((authorization_code, code_verifier)) => Ok(CodexDevicePollResponse {
            status: "approved".to_string(),
            authorization_code: Some(authorization_code),
            code_verifier: Some(code_verifier),
        }),
        None => Ok(CodexDevicePollResponse {
            status: "pending".to_string(),
            authorization_code: None,
            code_verifier: None,
        }),
    }
}

/// Exchange Codex authorization code for tokens and store credentials for GUI.
#[tracing::instrument(skip(authorization_code, code_verifier))]
#[tauri::command]
pub async fn codex_device_exchange_gui(
    authorization_code: String,
    code_verifier: String,
) -> Result<CodexDeviceExchangeResponse, String> {
    let tokens = oauth::codex_exchange_code(&authorization_code, &code_verifier)
        .await
        .map_err(|e| format!("Failed to exchange Codex auth code: {}", e))?;

    let store = CredentialStore::open()
        .await
        .map_err(|e| format!("Failed to open credential store: {}", e))?;

    store
        .store_codex_oauth(&CodexOAuthCredentials {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_at,
            account_id: tokens.account_id,
        })
        .await
        .map_err(|e| format!("Failed to store Codex tokens: {}", e))?;

    Ok(CodexDeviceExchangeResponse {
        status: "ok".to_string(),
        expires_at: tokens.expires_at,
    })
}

/// Open a URL in the system's default browser.
#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), String> {
    // Validate that it's an http(s) URL to prevent command injection
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Only http and https URLs are supported".to_string());
    }
    std::process::Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .map_err(|e| format!("Failed to open URL: {}", e))?;
    Ok(())
}
