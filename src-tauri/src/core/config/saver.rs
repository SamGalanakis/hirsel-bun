//! Configuration file saving and TOML generation.

use std::fs;
use std::path::Path;

use super::{Config, ConfigError, LlmProvider, S3Config, StorageBackend, StorageProvider};
use serde::Serialize;

/// Save the configuration to a TOML file.
///
/// This serializes the config back to TOML format and writes it to disk.
/// Note: Comments in the original file will be lost.
pub fn save_config(config: &Config, config_path: &Path) -> Result<(), ConfigError> {
    let mut output = String::new();

    // Agent section
    output.push_str("[agent]\n");
    let cmd_parts: Vec<String> = config
        .agent
        .command
        .iter()
        .map(|s| format!("\"{}\"", s))
        .collect();
    output.push_str(&format!("command = [{}]\n\n", cmd_parts.join(", ")));

    // Top-level settings
    output.push_str(&format!("eval_timeout = {}\n", config.eval_timeout));
    output.push_str(&format!(
        "human_in_the_loop = {}\n",
        config.human_in_the_loop
    ));
    output.push_str(&format!(
        "context_warning_threshold = {}\n",
        config.context_warning_threshold
    ));
    output.push_str(&format!("coordinator_port = {}\n", config.coordinator_port));
    output.push_str(&format!(
        "scribe_batch_window_seconds = {}\n",
        config.scribe_batch_window_seconds
    ));
    output.push('\n');

    // Backend connection
    write_backend_section(&mut output, config);

    // LLM section
    write_llm_section(&mut output, config);

    // MCP server imports
    write_mcp_section(&mut output, config)?;

    // Sandbox section
    write_sandbox_section(&mut output, config);

    // Storage section (only write if non-default)
    write_storage_section(&mut output, config);

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
    output.push_str("[llm]\n");
    output.push_str(&format!(
        "provider = \"{}\"\n",
        match config.llm.provider {
            LlmProvider::Codex => "codex",
            LlmProvider::Openrouter => "openrouter",
        }
    ));
    if let Some(base_url) = &config.llm.openrouter_base_url {
        output.push_str(&format!("openrouter_base_url = \"{}\"\n", base_url));
    }
    output.push('\n');
}

fn write_backend_section(output: &mut String, config: &Config) {
    output.push_str("[backend]\n");
    if let Some(url) = &config.backend.url {
        output.push_str(&format!("url = \"{}\"\n", url));
    }
    if let Some(api_key) = &config.backend.api_key {
        output.push_str(&format!("api_key = \"{}\"\n", api_key));
    }
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
    if let Some(ref container) = config.sandbox.container {
        output.push_str("[sandbox.container]\n");
        output.push_str(&format!("image = \"{}\"\n\n", container.image));
    }
}

fn write_s3_config(output: &mut String, s3: &S3Config) {
    // Write provider if not default (S3)
    if s3.provider != StorageProvider::S3 {
        output.push_str(&format!(
            "provider = \"{}\"\n",
            match s3.provider {
                StorageProvider::S3 => "s3",
                StorageProvider::Minio => "minio",
            }
        ));
    }
    if let Some(ref endpoint) = s3.endpoint {
        output.push_str(&format!("endpoint = \"{}\"\n", endpoint));
    }
    output.push_str(&format!("bucket = \"{}\"\n", s3.bucket));
    if let Some(ref region) = s3.region {
        output.push_str(&format!("region = \"{}\"\n", region));
    }
    if let Some(ref key) = s3.access_key_id {
        output.push_str(&format!("access_key_id = \"{}\"\n", key));
    }
    if let Some(ref secret) = s3.secret_access_key {
        output.push_str(&format!("secret_access_key = \"{}\"\n", secret));
    }
}

fn write_storage_section(output: &mut String, config: &Config) {
    let has_content = config.storage.files != StorageBackend::Local
        || !config.storage.storages.is_empty()
        || config.storage.default_storage.is_some();

    if has_content {
        output.push_str("[storage]\n");
        output.push_str(&format!(
            "files = \"{}\"\n",
            match config.storage.files {
                StorageBackend::Local => "local",
                StorageBackend::S3 => "s3",
            }
        ));

        if let Some(ref default_storage) = config.storage.default_storage {
            output.push_str(&format!("default_storage = \"{}\"\n", default_storage));
        }

        // Write named storages
        for (name, s3) in &config.storage.storages {
            output.push_str(&format!("\n[storage.storages.{}]\n", name));
            write_s3_config(output, s3);
        }

        output.push('\n');
    }
}
