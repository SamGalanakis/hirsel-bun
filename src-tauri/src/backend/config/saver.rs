//! Configuration file saving and TOML generation.

use std::fs;
use std::path::Path;

use super::{Config, ConfigError};
use serde::Serialize;

/// Save the configuration to a TOML file.
///
/// This serializes the config back to TOML format and writes it to disk.
/// Note: Comments in the original file will be lost.
pub fn save_config(config: &Config, config_path: &Path) -> Result<(), ConfigError> {
    let mut output = String::new();

    // Backend connection
    write_backend_section(&mut output, config);

    // LLM section
    write_llm_section(&mut output, config);

    // MCP server imports
    write_mcp_section(&mut output, config)?;

    // Sandbox section
    write_sandbox_section(&mut output, config);

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| ConfigError::ReadError {
            path: parent.to_path_buf(),
            message: e.to_string(),
        })?;
    }

    // Write config file
    fs::write(config_path, output).map_err(|e| {
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

    Ok(())
}

fn write_llm_section(output: &mut String, config: &Config) {
    #[derive(Serialize)]
    struct LlmSection<'a> {
        llm: &'a super::LlmConfig,
    }

    let section = toml::to_string_pretty(&LlmSection { llm: &config.llm })
        .expect("serializing llm config should succeed");
    output.push_str(&section);
    output.push('\n');
}

fn write_backend_section(output: &mut String, config: &Config) {
    if config.backend.url.is_none() && config.backend.api_key.is_none() {
        return;
    }

    #[derive(Serialize)]
    struct BackendSection<'a> {
        backend: &'a super::BackendConfig,
    }

    let section = toml::to_string_pretty(&BackendSection {
        backend: &config.backend,
    })
    .expect("serializing backend config should succeed");
    output.push_str(&section);
    output.push('\n');
}

fn write_mcp_section(output: &mut String, config: &Config) -> Result<(), ConfigError> {
    if config.mcp_servers.is_empty() {
        return Ok(());
    }

    #[derive(Serialize)]
    struct McpServersSection<'a> {
        mcp_servers: &'a std::collections::BTreeMap<String, super::McpServerConfig>,
    }

    let section = toml::to_string_pretty(&McpServersSection {
        mcp_servers: &config.mcp_servers,
    })
    .map_err(|e| ConfigError::ValidationError(format!("failed to serialize MCP servers: {}", e)))?;
    output.push_str(&section);
    output.push('\n');
    Ok(())
}

fn write_sandbox_section(output: &mut String, config: &Config) {
    if config.sandbox.image == crate::backend::sandbox::DEFAULT_WORKER_IMAGE {
        return;
    }

    #[derive(Serialize)]
    struct SandboxSection<'a> {
        sandbox: &'a crate::backend::sandbox::SandboxConfig,
    }

    let section = toml::to_string_pretty(&SandboxSection {
        sandbox: &config.sandbox,
    })
    .expect("serializing sandbox config should succeed");
    output.push_str(&section);
    output.push('\n');
}

#[cfg(test)]
mod tests {
    use super::save_config;
    use crate::backend::config::loader::load_config_file;
    use crate::backend::config::{
        AgentModelOverrides, BackendConfig, Config, LlmConfig, LlmProvider,
    };
    use crate::backend::sandbox::SandboxConfig;
    use tempfile::TempDir;

    #[test]
    fn save_config_round_trips_current_sections() {
        let temp = TempDir::new().expect("temp dir");
        let config_path = temp.path().join("config.toml");

        let config = Config {
            root: temp.path().to_path_buf(),
            llm: LlmConfig {
                provider: LlmProvider::Openrouter,
                openrouter_base_url: Some("https://openrouter.example/api".to_string()),
                model: Some("gpt-5".to_string()),
                model_variant: Some("high".to_string()),
                role_models: None,
                agent_models: Some(AgentModelOverrides {
                    low: Some("gpt-5-mini".to_string()),
                    medium: None,
                    high: Some("gpt-5".to_string()),
                }),
            },
            sandbox: SandboxConfig {
                image: "custom/worker:latest".to_string(),
            },
            backend: BackendConfig {
                url: Some("http://127.0.0.1:8080".to_string()),
                api_key: Some("dev-test-key".to_string()),
            },
            mcp_servers: Default::default(),
        };

        save_config(&config, &config_path).expect("save config");
        let mut loaded = Config::default();
        let warnings = load_config_file(&mut loaded, &config_path).expect("load config");

        assert!(warnings.is_empty());
        assert_eq!(loaded.backend.url, config.backend.url);
        assert_eq!(loaded.backend.api_key, config.backend.api_key);
        assert_eq!(loaded.llm.provider, config.llm.provider);
        assert_eq!(
            loaded.llm.openrouter_base_url,
            config.llm.openrouter_base_url
        );
        assert_eq!(loaded.llm.model, config.llm.model);
        assert_eq!(loaded.llm.model_variant, config.llm.model_variant);
        assert_eq!(
            loaded.llm.agent_models.as_ref().and_then(|m| m.low.clone()),
            Some("gpt-5-mini".to_string())
        );
        assert_eq!(
            loaded
                .llm
                .agent_models
                .as_ref()
                .and_then(|m| m.high.clone()),
            Some("gpt-5".to_string())
        );
        assert_eq!(loaded.sandbox.image, config.sandbox.image);
    }
}
