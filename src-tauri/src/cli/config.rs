//! Agent runtime command configuration helpers.

use std::path::PathBuf;

/// Agent preset configuration
#[derive(Debug, Clone)]
pub struct AgentPreset {
    /// Command to run the agent
    pub command: Vec<String>,
    /// Human-readable description
    pub description: &'static str,
    /// Install hint if command not found
    pub install_hint: Option<&'static str>,
}

fn config_file_path() -> PathBuf {
    crate::core::config::hirsel_dir().join("config.toml")
}

fn read_config_file() -> Option<toml::Table> {
    let path = config_file_path();
    if !path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&path).ok()?;
    content.parse::<toml::Table>().ok()
}

/// Get the configured agent command, defaulting to the lash worker runtime if not set.
pub fn get_agent_command() -> Vec<String> {
    if let Some(config) = read_config_file() {
        if let Some(agent) = config.get("agent").and_then(|a| a.as_table()) {
            if let Some(command) = agent.get("command").and_then(|c| c.as_array()) {
                let cmd: Vec<String> = command
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if !cmd.is_empty() {
                    return cmd;
                }
            }
        }
    }

    let hirsel_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "hirsel".to_string());
    vec![hirsel_path, "__worker-run".to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_file_path() {
        let path = config_file_path();
        assert!(path.ends_with("config.toml"));
    }
}
