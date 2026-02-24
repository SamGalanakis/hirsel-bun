//! Configuration-related commands
//!
//! Commands for reading and writing application configuration.

use super::types::{ConfigDefaults, ConfigUpdateRequest};
use super::ResultExt;
use crate::core::api_types::ConfigResponse;
use crate::core::config;
use crate::core::credentials::{CodexOAuthCredentials, CredentialStore};
use crate::core::orchestrator::create_orchestrator;
use crate::core::tailscale::{get_tailscale_status, is_tailscale_connected};
use lash_core::oauth;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Instant;

/// Get application configuration
/// Uses the orchestrator to support both local and remote modes
#[tracing::instrument]
#[tauri::command]
pub async fn get_config() -> Result<ConfigResponse, String> {
    let orch = create_orchestrator(None).str_err()?;
    orch.get_config().await.str_err()
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
    if let Some(pause) = updates.user_message_pause {
        cfg.user_message_pause = pause;
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

    // Apply profiles updates (replace entire map if provided)
    if let Some(profiles) = updates.profiles {
        cfg.profiles = profiles.into_iter().map(|(k, v)| (k, v.into())).collect();
    }

    // Apply default_profile update
    if let Some(default_profile) = updates.default_profile {
        cfg.default_profile = default_profile;
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

/// Tailscale connection info for the "This Machine" feature
#[derive(Serialize)]
pub struct TailscaleInfo {
    pub connected: bool,
    pub hostname: Option<String>,
    pub dns_name: Option<String>,
    pub tailscale_ips: Vec<String>,
}

/// Get Tailscale connection info for this machine
#[tracing::instrument]
#[tauri::command]
pub fn get_tailscale_info() -> Result<TailscaleInfo, String> {
    if !is_tailscale_connected() {
        return Ok(TailscaleInfo {
            connected: false,
            hostname: None,
            dns_name: None,
            tailscale_ips: vec![],
        });
    }

    match get_tailscale_status() {
        Ok(status) => Ok(TailscaleInfo {
            connected: true,
            hostname: Some(status.self_node.hostname),
            dns_name: Some(status.self_node.dns_name),
            tailscale_ips: status.self_node.tailscale_ips,
        }),
        Err(e) => Err(e.to_string()),
    }
}

/// SSH connection check result
#[derive(Serialize)]
pub struct SshCheckResult {
    pub reachable: bool,
    pub error: Option<String>,
    pub latency_ms: Option<u64>,
}

/// Check if an SSH runner is reachable
///
/// Runs: ssh -o BatchMode=yes -o ConnectTimeout=5 -o StrictHostKeyChecking=accept-new {host} echo ok
#[tracing::instrument]
#[tauri::command]
pub async fn check_ssh_runner(
    host: String,
    port: u16,
    ssh_key: Option<String>,
) -> Result<SshCheckResult, String> {
    let start = Instant::now();

    let mut cmd = Command::new("ssh");

    // Basic SSH options for non-interactive check
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=5",
        "-o",
        "StrictHostKeyChecking=accept-new",
    ]);

    // Add port if not default
    if port != 22 {
        cmd.args(["-p", &port.to_string()]);
    }

    // Add SSH key if provided
    if let Some(key) = ssh_key {
        if !key.is_empty() {
            cmd.args(["-i", &key]);
        }
    }

    // Add host and command
    cmd.arg(&host);
    cmd.arg("echo");
    cmd.arg("ok");

    // Run the command
    let output = cmd.output();

    let latency_ms = start.elapsed().as_millis() as u64;

    match output {
        Ok(output) => {
            if output.status.success() {
                Ok(SshCheckResult {
                    reachable: true,
                    error: None,
                    latency_ms: Some(latency_ms),
                })
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Extract meaningful error message
                let error_msg = if stderr.contains("Permission denied") {
                    "Permission denied".to_string()
                } else if stderr.contains("Connection refused") {
                    "Connection refused".to_string()
                } else if stderr.contains("Connection timed out")
                    || stderr.contains("Operation timed out")
                {
                    "Connection timed out".to_string()
                } else if stderr.contains("Could not resolve hostname") {
                    "Host not found".to_string()
                } else if stderr.is_empty() {
                    "SSH connection failed".to_string()
                } else {
                    stderr.lines().next().unwrap_or("SSH error").to_string()
                };

                Ok(SshCheckResult {
                    reachable: false,
                    error: Some(error_msg),
                    latency_ms: Some(latency_ms),
                })
            }
        }
        Err(e) => Ok(SshCheckResult {
            reachable: false,
            error: Some(format!("Failed to run ssh: {}", e)),
            latency_ms: None,
        }),
    }
}
