//! Configuration-related commands
//!
//! Commands for reading and writing application configuration.

use super::types::{ConfigResponse, ConfigUpdateRequest};
use crate::core::config;
use crate::core::orchestrator::create_orchestrator;

/// Get application configuration
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn get_config() -> Result<ConfigResponse, String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.get_config().await.map_err(|e| e.to_string())
}

/// Save application configuration
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
    if let Some(max) = updates.max_iterations {
        cfg.max_iterations = max;
    }
    if let Some(pause) = updates.user_message_pause {
        cfg.user_message_pause = pause;
    }
    if let Some(hitl) = updates.human_in_the_loop {
        cfg.human_in_the_loop = hitl;
    }
    if let Some(enabled) = updates.compaction_enabled {
        cfg.compaction_enabled = enabled;
    }
    if let Some(threshold) = updates.compaction_threshold {
        cfg.compaction_threshold = threshold;
    }
    if let Some(keep) = updates.compaction_keep_messages {
        cfg.compaction_keep_messages = keep;
    }
    if let Some(auto) = updates.auto_improve {
        cfg.auto_improve = auto;
    }
    if let Some(warning) = updates.context_warning_threshold {
        cfg.context_warning_threshold = warning;
    }
    if let Some(port) = updates.coordinator_port {
        cfg.coordinator_port = port;
    }

    // Apply auth updates
    if let Some(auth_update) = updates.auth {
        if let Some(method) = auth_update.default_method {
            cfg.auth.default_method = method.into();
        }
        if let Some(claude) = auth_update.claude {
            cfg.auth.claude = Some(claude.into());
        }
        if let Some(gemini) = auth_update.gemini {
            cfg.auth.gemini = Some(gemini.into());
        }
        if let Some(codex) = auth_update.codex {
            cfg.auth.codex = Some(codex.into());
        }
        if let Some(goose) = auth_update.goose {
            cfg.auth.goose = Some(goose.into());
        }
    }

    // Apply remotes updates (replace entire map if provided)
    if let Some(remotes) = updates.remotes {
        cfg.remotes = remotes.into_iter().map(|(k, v)| (k, v.into())).collect();
    }

    // Apply default_remote update
    if let Some(default_remote) = updates.default_remote {
        cfg.default_remote = default_remote;
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

    // Serialize to TOML
    let toml_str =
        toml::to_string_pretty(&cfg).map_err(|e| format!("Failed to serialize config: {}", e))?;

    // Write to file
    std::fs::write(&config_path, toml_str).map_err(|e| format!("Failed to write config: {}", e))?;

    Ok(())
}
