//! Configuration-related commands
//!
//! Commands for reading and writing application configuration.

use super::types::{ConfigDefaults, ConfigUpdateRequest};
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

/// Get global config defaults for project settings inheritance
///
/// Returns default values that projects inherit when they don't have
/// project-specific settings. This allows the UI to show what values
/// will be used when a project setting is empty.
#[tracing::instrument]
#[tauri::command]
pub async fn get_config_defaults() -> Result<ConfigDefaults, String> {
    let (cfg, _) = config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));

    Ok(ConfigDefaults {
        worker_scale: "5".to_string(),
        time_limit_minutes: None,
        human_in_the_loop: cfg.human_in_the_loop,
        runners: cfg.runner_names(),
        default_runner: cfg.default_runner.clone(),
    })
}

/// Save application configuration
#[tracing::instrument(skip(updates))]
#[tauri::command]
pub async fn save_config(updates: ConfigUpdateRequest) -> Result<(), String> {
    let config_path = config::hirsel_dir().join("config.toml");

    // Load existing config or create default
    let (mut cfg, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));

    // Apply updates
    if let Some(cmd) = updates.agent_command {
        cfg.agent.command = cmd;
    }
    if let Some(timeout) = updates.eval_timeout {
        cfg.eval_timeout = timeout;
    }
    if let Some(auto) = updates.auto_learn {
        cfg.auto_learn = auto;
    }
    if let Some(hitl) = updates.human_in_the_loop {
        cfg.human_in_the_loop = hitl;
    }
    if let Some(warning) = updates.context_warning_threshold {
        cfg.context_warning_threshold = warning;
    }
    if let Some(port) = updates.coordinator_port {
        cfg.coordinator_port = port;
    }

    // Apply LLM updates
    if let Some(llm_update) = updates.llm {
        llm_update.apply(&mut cfg.llm);
    }

    // Apply runners updates (replace entire map if provided)
    if let Some(runners) = updates.runners {
        cfg.runners = runners.into_iter().map(|(k, v)| (k, v.into())).collect();
    }

    // Apply default_runner update
    if let Some(default_runner) = updates.default_runner {
        cfg.default_runner = default_runner;
    }

    // Apply worker_runners update
    if let Some(worker_runners) = updates.worker_runners {
        cfg.worker_runners = worker_runners;
    }

    // Apply backend connection update
    if let Some(backend) = updates.backend {
        cfg.backend = backend.into();
    }

    // Apply git config update
    if let Some(git_update) = updates.git {
        cfg.git = git_update.into();
    }

    // Apply storage config update
    if let Some(storage_update) = updates.storage {
        cfg.storage = storage_update.into();
    }

    // Serialize to TOML
    let toml_str = toml::to_string_pretty(&cfg).context("Failed to serialize config")?;

    // Write to file
    std::fs::write(&config_path, toml_str).context("Failed to write config")?;

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
