//! Configuration file loading and TOML parsing.

use std::fs;
use std::path::Path;

use super::{
    AgentAuth, AuthConfig, AuthMethod, Config, ConfigError, GitConfig, GitProvider,
    OrchestratorAccess, OrchestratorMode, OrchestratorProfile, S3Config, StorageBackend,
    StorageConfig, StorageProvider,
};

/// Parse an S3Config from a TOML table
fn parse_s3_config(table: &toml::Table) -> S3Config {
    let provider = table
        .get("provider")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "tigris" => StorageProvider::Tigris,
            _ => StorageProvider::S3,
        })
        .unwrap_or(StorageProvider::S3);

    S3Config {
        provider,
        endpoint: table
            .get("endpoint")
            .and_then(|v| v.as_str())
            .map(String::from),
        bucket: table
            .get("bucket")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        region: table
            .get("region")
            .and_then(|v| v.as_str())
            .map(String::from),
        access_key_id: table
            .get("access_key_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        secret_access_key: table
            .get("secret_access_key")
            .and_then(|v| v.as_str())
            .map(String::from),
    }
}

/// Load settings from a TOML config file into the Config struct.
/// Returns warnings for non-fatal issues.
pub fn load_config_file(
    config: &mut Config,
    config_path: &Path,
) -> Result<Vec<String>, ConfigError> {
    let mut warnings = Vec::new();

    if !config_path.exists() {
        return Ok(warnings);
    }

    let content = fs::read_to_string(config_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            ConfigError::PermissionDenied {
                path: config_path.to_path_buf(),
            }
        } else {
            ConfigError::ReadError {
                path: config_path.to_path_buf(),
                message: e.to_string(),
            }
        }
    })?;

    let table: toml::Table =
        content
            .parse()
            .map_err(|e: toml::de::Error| ConfigError::InvalidToml {
                path: config_path.to_path_buf(),
                message: e.to_string(),
            })?;

    // Load agent config
    if let Some(agent_data) = table.get("agent") {
        if let Some(agent_table) = agent_data.as_table() {
            if let Some(cmd) = agent_table.get("command") {
                if let Some(arr) = cmd.as_array() {
                    let command: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                    if !command.is_empty() {
                        config.agent.command = command;
                    }
                } else {
                    warnings.push(format!(
                        "Config warning: agent.command should be a list of strings, got {}",
                        cmd.type_str()
                    ));
                }
            }
        } else {
            warnings.push(format!(
                "Config warning: [agent] section should be a table, got {}",
                agent_data.type_str()
            ));
        }
    }

    // Load eval_timeout
    if let Some(val) = table.get("eval_timeout") {
        if let Some(timeout) = val.as_integer() {
            let timeout = timeout as u32;
            if timeout < 60 {
                warnings.push(format!(
                    "Config warning: eval_timeout={} is below minimum (60s), using 60s",
                    timeout
                ));
                config.eval_timeout = 60;
            } else if timeout > 7200 {
                warnings.push(format!(
                    "Config warning: eval_timeout={} exceeds maximum (7200s), using 7200s",
                    timeout
                ));
                config.eval_timeout = 7200;
            } else {
                config.eval_timeout = timeout;
            }
        } else {
            warnings.push(format!(
                "Config warning: invalid eval_timeout value: {}",
                val
            ));
        }
    }

    // Load auto_learn
    if let Some(val) = table.get("auto_learn") {
        if let Some(b) = val.as_bool() {
            config.auto_learn = b;
        } else {
            warnings.push(format!(
                "Config warning: auto_learn should be a boolean, got {}",
                val.type_str()
            ));
        }
    }

    // Load human_in_the_loop
    if let Some(val) = table.get("human_in_the_loop") {
        if let Some(b) = val.as_bool() {
            config.human_in_the_loop = b;
        } else {
            warnings.push(format!(
                "Config warning: human_in_the_loop should be a boolean, got {}",
                val.type_str()
            ));
        }
    }

    // Load user_message_pause
    if let Some(val) = table.get("user_message_pause") {
        if let Some(s) = val.as_str() {
            if s == "sender" || s == "all" {
                config.user_message_pause = s.to_string();
            } else {
                warnings.push(format!(
                    "Config warning: user_message_pause must be 'sender' or 'all', got {}",
                    s
                ));
            }
        }
    }

    // Load coordinator_port
    if let Some(val) = table.get("coordinator_port") {
        if let Some(n) = val.as_integer() {
            if n > 0 {
                config.coordinator_port = n as u16;
            } else {
                warnings.push(format!(
                    "Config warning: coordinator_port must be a positive integer, got {}",
                    n
                ));
            }
        }
    }

    // Load context_warning_threshold
    if let Some(val) = table.get("context_warning_threshold") {
        if let Some(n) = val.as_float() {
            if (0.0..=1.0).contains(&n) {
                config.context_warning_threshold = n;
            } else {
                warnings.push(format!(
                    "Config warning: context_warning_threshold must be between 0.0 and 1.0, got {}",
                    n
                ));
            }
        } else if let Some(n) = val.as_integer() {
            let n = n as f64;
            if (0.0..=1.0).contains(&n) {
                config.context_warning_threshold = n;
            }
        }
    }

    // Load auth configuration
    load_auth_config(&table, &mut config.auth, &mut warnings);

    // Load runners configuration
    if let Some(runners_data) = table.get("runners") {
        if let Some(runners_table) = runners_data.as_table() {
            for (name, runner_data) in runners_table {
                // Serialize TOML value to string, then parse as RunnerConfig
                let toml_str = toml::to_string(runner_data).unwrap_or_default();
                match toml::from_str::<crate::core::runner::RunnerConfig>(&toml_str) {
                    Ok(runner_config) => {
                        config.runners.insert(name.clone(), runner_config);
                    }
                    Err(e) => {
                        warnings.push(format!(
                            "Config warning: [runners.{}] invalid config: {}",
                            name, e
                        ));
                    }
                }
            }
        }
    }

    // Load default_runner
    if let Some(val) = table.get("default_runner") {
        if let Some(s) = val.as_str() {
            if s == "local" || s.is_empty() {
                config.default_runner = None;
            } else {
                config.default_runner = Some(s.to_string());
            }
        }
    }

    // Load worker_runners assignments
    if let Some(worker_runners_data) = table.get("worker_runners") {
        if let Some(worker_runners_table) = worker_runners_data.as_table() {
            for (worker_name, runner_name_val) in worker_runners_table {
                if let Some(runner_name) = runner_name_val.as_str() {
                    config
                        .worker_runners
                        .insert(worker_name.clone(), runner_name.to_string());
                }
            }
        }
    }

    // Load default_profile
    if let Some(val) = table.get("default_profile") {
        if let Some(s) = val.as_str() {
            config.default_profile = s.to_string();
        }
    }

    // Load orchestrator profiles
    load_profiles(&table, &mut config.profiles, &mut warnings);

    // Load git configuration
    load_git_config(&table, &mut config.git, &mut warnings);

    // Load storage configuration
    load_storage_config(&table, &mut config.storage, &mut warnings);

    Ok(warnings)
}

fn load_auth_config(table: &toml::Table, auth: &mut AuthConfig, _warnings: &mut Vec<String>) {
    if let Some(auth_data) = table.get("auth") {
        if let Some(auth_table) = auth_data.as_table() {
            if let Some(val) = auth_table.get("default_method") {
                if let Some(s) = val.as_str() {
                    if let Ok(method) = s.parse::<AuthMethod>() {
                        auth.default_method = method;
                    }
                }
            }

            for agent_name in &["claude", "gemini", "codex", "goose"] {
                if let Some(agent_auth_data) = auth_table.get(*agent_name) {
                    if let Some(agent_auth_table) = agent_auth_data.as_table() {
                        let method_str = agent_auth_table
                            .get("method")
                            .and_then(|v| v.as_str())
                            .unwrap_or("env");

                        if let Ok(method) = method_str.parse::<AuthMethod>() {
                            let agent_auth = AgentAuth {
                                method,
                                api_key: agent_auth_table
                                    .get("api_key")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                                env_var: agent_auth_table
                                    .get("env_var")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                            };
                            match *agent_name {
                                "claude" => auth.claude = Some(agent_auth),
                                "gemini" => auth.gemini = Some(agent_auth),
                                "codex" => auth.codex = Some(agent_auth),
                                "goose" => auth.goose = Some(agent_auth),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
}

fn load_profiles(
    table: &toml::Table,
    profiles: &mut std::collections::HashMap<String, OrchestratorProfile>,
    warnings: &mut Vec<String>,
) {
    if let Some(profiles_data) = table.get("profiles") {
        if let Some(profiles_table) = profiles_data.as_table() {
            for (name, profile_data) in profiles_table {
                if let Some(profile_table) = profile_data.as_table() {
                    let mode_str = profile_table
                        .get("mode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("local");

                    let mode = match mode_str {
                        "remote" => OrchestratorMode::Remote,
                        _ => OrchestratorMode::Local,
                    };

                    // Parse access strategy
                    let access = if let Some(access_data) = profile_table.get("access") {
                        if let Some(access_table) = access_data.as_table() {
                            let access_type = access_table
                                .get("type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("direct");

                            match access_type {
                                "tailscale" => {
                                    let client_id = access_table
                                        .get("oauth_client_id")
                                        .and_then(|v| v.as_str());
                                    let client_secret = access_table
                                        .get("oauth_client_secret")
                                        .and_then(|v| v.as_str());
                                    let tag = access_table
                                        .get("tag")
                                        .and_then(|v| v.as_str())
                                        .map(String::from);

                                    if let (Some(id), Some(secret)) = (client_id, client_secret) {
                                        OrchestratorAccess::Tailscale {
                                            oauth_client_id: id.to_string(),
                                            oauth_client_secret: secret.to_string(),
                                            tag,
                                        }
                                    } else {
                                        warnings.push(format!(
                                            "Config warning: [profiles.{}.access] tailscale requires 'oauth_client_id' and 'oauth_client_secret'",
                                            name
                                        ));
                                        OrchestratorAccess::Direct
                                    }
                                }
                                _ => OrchestratorAccess::Direct,
                            }
                        } else {
                            OrchestratorAccess::Direct
                        }
                    } else {
                        OrchestratorAccess::Direct
                    };

                    let profile = OrchestratorProfile {
                        mode,
                        url: profile_table
                            .get("url")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        api_key: profile_table
                            .get("api_key")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        access,
                    };

                    // Validate remote profiles have required fields
                    if mode == OrchestratorMode::Remote {
                        if profile.url.is_none() {
                            warnings.push(format!(
                                "Config warning: [profiles.{}] remote mode requires 'url' field",
                                name
                            ));
                            continue;
                        }
                        // Note: api_key is optional in config - it can be loaded from credential store
                    }

                    profiles.insert(name.clone(), profile);
                }
            }
        }
    }
}

fn load_git_config(table: &toml::Table, git: &mut GitConfig, warnings: &mut Vec<String>) {
    if let Some(git_data) = table.get("git") {
        if let Some(git_table) = git_data.as_table() {
            if let Some(provider_str) = git_table.get("default_provider").and_then(|v| v.as_str()) {
                git.default_provider = match provider_str.to_lowercase().as_str() {
                    "github" => Some(GitProvider::Github),
                    _ => {
                        warnings.push(format!(
                            "Config warning: unknown git provider '{}', ignoring",
                            provider_str
                        ));
                        None
                    }
                };
            }
        }
    }
}

fn load_storage_config(
    table: &toml::Table,
    storage: &mut StorageConfig,
    warnings: &mut Vec<String>,
) {
    if let Some(storage_data) = table.get("storage") {
        if let Some(storage_table) = storage_data.as_table() {
            // Parse files backend
            if let Some(files_str) = storage_table.get("files").and_then(|v| v.as_str()) {
                storage.files = match files_str.to_lowercase().as_str() {
                    "local" => StorageBackend::Local,
                    "s3" => StorageBackend::S3,
                    _ => {
                        warnings.push(format!(
                            "Config warning: unknown storage backend '{}', using local",
                            files_str
                        ));
                        StorageBackend::Local
                    }
                };
            }

            // Parse default_storage
            if let Some(default) = storage_table
                .get("default_storage")
                .and_then(|v| v.as_str())
            {
                storage.default_storage = Some(default.to_string());
            }

            // Parse named storages: [storage.storages.name]
            if let Some(storages_data) = storage_table.get("storages") {
                if let Some(storages_table) = storages_data.as_table() {
                    for (name, value) in storages_table {
                        if let Some(s3_table) = value.as_table() {
                            storage
                                .storages
                                .insert(name.clone(), parse_s3_config(s3_table));
                        }
                    }
                }
            }
        }
    }
}
