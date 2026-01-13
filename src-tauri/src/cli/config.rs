//! CLI config command implementation.
//!
//! Provides agent selection and configuration management for hirsel.
//! Supports both interactive mode (curses-style menu) and direct agent setting.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

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

/// Available agent presets
pub fn agent_presets() -> HashMap<&'static str, AgentPreset> {
    let mut presets = HashMap::new();

    presets.insert(
        "claude",
        AgentPreset {
            command: vec!["claude-code-acp".into()],
            description: "Anthropic Claude Code",
            install_hint: Some("npm install -g @anthropics/claude-code-acp"),
        },
    );

    presets.insert(
        "gemini",
        AgentPreset {
            command: vec!["gemini".into()],
            description: "Google Gemini CLI",
            install_hint: None,
        },
    );

    presets.insert(
        "opencode",
        AgentPreset {
            command: vec!["opencode".into(), "acp".into()],
            description: "OpenCode",
            install_hint: None,
        },
    );

    presets.insert(
        "codex",
        AgentPreset {
            command: vec!["codex".into()],
            description: "OpenAI Codex CLI",
            install_hint: None,
        },
    );

    presets.insert(
        "goose",
        AgentPreset {
            command: vec!["goose".into()],
            description: "Block Goose",
            install_hint: None,
        },
    );

    presets
}

/// Get the hirsel root directory (~/.hirsel)
pub fn hirsel_root() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".hirsel"))
        .unwrap_or_else(|| PathBuf::from(".hirsel"))
}

/// Get the config file path (~/.hirsel/config.toml)
pub fn config_file_path() -> PathBuf {
    hirsel_root().join("config.toml")
}

/// Read the current config file and parse agent settings
pub fn read_config_file() -> Option<toml::Table> {
    let path = config_file_path();
    if !path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&path).ok()?;
    content.parse::<toml::Table>().ok()
}

/// Write agent command to config file
pub fn write_config_file(command: &[String]) -> io::Result<()> {
    let root = hirsel_root();
    std::fs::create_dir_all(&root)?;

    let cmd_str = command
        .iter()
        .map(|c| format!("\"{}\"", c))
        .collect::<Vec<_>>()
        .join(", ");

    let content = format!("[agent]\ncommand = [{}]\n", cmd_str);
    std::fs::write(config_file_path(), content)
}

/// Get the currently configured agent name (if it matches a preset)
pub fn get_current_agent() -> Option<String> {
    let config = read_config_file()?;
    let agent = config.get("agent")?.as_table()?;
    let command = agent.get("command")?.as_array()?;

    let cmd: Vec<String> = command
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let presets = agent_presets();
    for (name, preset) in &presets {
        if preset.command == cmd {
            return Some(name.to_string());
        }
    }
    None
}

/// Get the configured agent command, defaulting to claude if not set
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
    // Default to claude
    vec!["claude-code-acp".to_string()]
}

/// Check if a command is available in PATH
pub fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run the config command - either interactive or direct agent setting
pub fn run_config(agent: Option<String>) -> Result<(), String> {
    match agent {
        None => run_interactive_config(),
        Some(name) => set_agent(&name),
    }
}

/// Set agent directly by name
pub fn set_agent(agent: &str) -> Result<(), String> {
    let agent = agent.to_lowercase();
    let presets = agent_presets();

    let preset = presets
        .get(agent.as_str())
        .ok_or_else(|| {
            let available = presets.keys().copied().collect::<Vec<_>>().join(", ");
            format!("Unknown agent: {}. Available: {}", agent, available)
        })?;

    // Check if command exists
    let cmd = &preset.command[0];
    if !command_exists(cmd) {
        let mut msg = format!("Command not found: {}", cmd);
        if let Some(hint) = preset.install_hint {
            msg.push_str(&format!("\n\nInstall with:\n  {}", hint));
        } else {
            msg.push_str(&format!("\n\nMake sure '{}' is installed and in your PATH", cmd));
        }
        return Err(msg);
    }

    // Write config
    write_config_file(&preset.command)
        .map_err(|e| format!("Failed to write config: {}", e))?;

    println!("\n✓ Agent set to {}", agent);
    println!("  {}", preset.description);

    Ok(())
}

/// Run interactive agent selection (simple fallback mode)
pub fn run_interactive_config() -> Result<(), String> {
    let presets = agent_presets();
    let agents: Vec<&str> = {
        let mut v: Vec<_> = presets.keys().copied().collect();
        v.sort();
        v
    };

    let current = get_current_agent();

    println!("Select AI Agent\n");

    for (i, name) in agents.iter().enumerate() {
        let preset = &presets[name];
        let marker = if Some(name.to_string()) == current {
            "●"
        } else {
            "○"
        };
        println!("  {} {} {:<12} {}", marker, i + 1, name, preset.description);
    }

    println!("\nEnter number or name (q to quit):");
    print!("> ");
    io::stdout().flush().map_err(|e| e.to_string())?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| e.to_string())?;

    let input = input.trim().to_lowercase();

    if input == "q" || input.is_empty() {
        println!("No changes made");
        return Ok(());
    }

    // Try as number first
    if let Ok(idx) = input.parse::<usize>() {
        if idx >= 1 && idx <= agents.len() {
            return set_agent(agents[idx - 1]);
        }
    }

    // Try as agent name
    if presets.contains_key(input.as_str()) {
        return set_agent(&input);
    }

    Err(format!("Invalid selection: {}", input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_presets() {
        let presets = agent_presets();
        assert!(presets.contains_key("claude"));
        assert!(presets.contains_key("gemini"));
        assert!(presets.contains_key("opencode"));
        assert!(presets.contains_key("codex"));
        assert!(presets.contains_key("goose"));

        let claude = &presets["claude"];
        assert_eq!(claude.command, vec!["claude-code-acp"]);
        assert_eq!(claude.description, "Anthropic Claude Code");
    }

    #[test]
    fn test_hirsel_root() {
        let root = hirsel_root();
        assert!(root.ends_with(".hirsel"));
    }

    #[test]
    fn test_config_file_path() {
        let path = config_file_path();
        assert!(path.ends_with("config.toml"));
    }
}
