//! Configuration file loading and TOML parsing.

use serde::de::DeserializeOwned;
use std::fs;
use std::path::Path;

use super::{Config, ConfigError, McpServerConfig};
use std::collections::{BTreeMap, BTreeSet};

fn validate_top_level_keys(table: &toml::Table) -> Result<(), ConfigError> {
    let allowed: BTreeSet<&str> = ["root", "llm", "sandbox", "backend", "mcp_servers"]
        .into_iter()
        .collect();

    let unknown = table
        .keys()
        .filter(|key| !allowed.contains(key.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    if unknown.is_empty() {
        return Ok(());
    }

    Err(ConfigError::ValidationError(format!(
        "unknown config keys: {}",
        unknown.join(", ")
    )))
}

fn parse_section<T: DeserializeOwned>(
    section_name: &str,
    value: &toml::Value,
) -> Result<T, ConfigError> {
    let toml_str = toml::to_string(value).unwrap_or_default();
    toml::from_str::<T>(&toml_str).map_err(|error| {
        ConfigError::ValidationError(format!("[{}] invalid config: {}", section_name, error))
    })
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

    validate_top_level_keys(&table)?;

    if let Some(root) = table.get("root").and_then(|value| value.as_str()) {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            config.root = trimmed.into();
        }
    }

    // Load LLM configuration
    if let Some(llm_data) = table.get("llm") {
        config.llm = parse_section("llm", llm_data)?;
    }

    // Load sandbox configuration
    if let Some(sandbox_data) = table.get("sandbox") {
        config.sandbox = parse_section("sandbox", sandbox_data)?;
    }

    // Load backend connection
    if let Some(backend_data) = table.get("backend") {
        config.backend = parse_section("backend", backend_data)?;
    }

    // Load MCP server imports
    load_mcp_servers(&table, &mut config.mcp_servers, &mut warnings);

    Ok(warnings)
}

fn load_mcp_servers(
    table: &toml::Table,
    mcp_servers: &mut BTreeMap<String, McpServerConfig>,
    warnings: &mut Vec<String>,
) {
    let Some(mcp_data) = table.get("mcp_servers") else {
        return;
    };
    let toml_str = toml::to_string(mcp_data).unwrap_or_default();
    match toml::from_str::<BTreeMap<String, McpServerConfig>>(&toml_str) {
        Ok(parsed) => *mcp_servers = parsed,
        Err(err) => warnings.push(format!(
            "Config warning: [mcp_servers] invalid config: {}",
            err
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::load_config_file;
    use crate::backend::config::{Config, ConfigError};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn rejects_legacy_agent_section() {
        let temp = TempDir::new().expect("temp dir");
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            "[agent]\ncommand = [\"hirsel\", \"__worker-run\"]\n",
        )
        .expect("write config");

        let mut config = Config::default();
        match load_config_file(&mut config, &config_path) {
            Err(ConfigError::ValidationError(message)) => {
                assert!(message.contains("unknown config keys: agent"));
            }
            other => panic!("expected validation error, got {:?}", other),
        }
    }

    #[test]
    fn loads_full_llm_section() {
        let temp = TempDir::new().expect("temp dir");
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
[llm]
provider = "openrouter"
openrouter_base_url = "https://openrouter.example/api"
model = "gpt-5"
model_variant = "high"

[llm.agent_models]
low = "gpt-5-mini"
high = "gpt-5"
"#,
        )
        .expect("write config");

        let mut config = Config::default();
        let warnings = load_config_file(&mut config, &config_path).expect("load config");

        assert!(warnings.is_empty());
        assert_eq!(
            config
                .llm
                .agent_models
                .as_ref()
                .and_then(|m| m.low.as_deref()),
            Some("gpt-5-mini")
        );
        assert_eq!(
            config
                .llm
                .agent_models
                .as_ref()
                .and_then(|m| m.high.as_deref()),
            Some("gpt-5")
        );
    }
}
