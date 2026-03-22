//! Configuration file loading and TOML parsing.

use std::fs;
use std::path::Path;

use super::{
    BackendConfig, Config, ConfigError, GitConfig, GitProvider, LlmConfig, LlmProvider, S3Config,
    StorageBackend, StorageConfig, StorageProvider,
};

/// Parse an S3Config from a TOML table
fn parse_s3_config(table: &toml::Table) -> S3Config {
    let provider = table
        .get("provider")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "minio" => StorageProvider::Minio,
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

    // Load LLM configuration
    load_llm_config(&table, &mut config.llm);

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

    // Load backend connection
    load_backend_config(&table, &mut config.backend, &mut warnings);

    // Load git configuration
    load_git_config(&table, &mut config.git, &mut warnings);

    // Load storage configuration
    load_storage_config(&table, &mut config.storage, &mut warnings);

    Ok(warnings)
}

fn load_llm_config(table: &toml::Table, llm: &mut LlmConfig) {
    if let Some(llm_data) = table.get("llm") {
        if let Some(llm_table) = llm_data.as_table() {
            if let Some(provider) = llm_table.get("provider").and_then(|v| v.as_str()) {
                llm.provider = match provider.to_lowercase().as_str() {
                    "openrouter" => LlmProvider::Openrouter,
                    _ => LlmProvider::Codex,
                };
            }
            llm.openrouter_base_url = llm_table
                .get("openrouter_base_url")
                .and_then(|v| v.as_str())
                .and_then(|v| {
                    let trimmed = v.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                });
        }
    }
}

fn load_backend_config(
    table: &toml::Table,
    backend: &mut BackendConfig,
    warnings: &mut Vec<String>,
) {
    if let Some(backend_data) = table.get("backend") {
        if let Some(backend_table) = backend_data.as_table() {
            backend.url = backend_table
                .get("url")
                .and_then(|v| v.as_str())
                .map(String::from);
            backend.api_key = backend_table
                .get("api_key")
                .and_then(|v| v.as_str())
                .map(String::from);
        } else {
            warnings.push("Config warning: [backend] must be a table".to_string());
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
