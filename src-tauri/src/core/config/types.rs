//! Agent type definitions.

use serde::{Deserialize, Serialize};

/// Type of AI agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Codex,
    #[default]
    Unknown,
}

impl AgentType {
    /// Detect agent type from command.
    pub fn from_command(command: &[String]) -> Self {
        if command.is_empty() {
            return Self::Unknown;
        }

        let full_cmd = command.join(" ").to_lowercase();
        if full_cmd.contains("codex") || full_cmd.contains("__worker-run") {
            Self::Codex
        } else {
            Self::Unknown
        }
    }

    /// Whether this agent type supports context tracking.
    pub fn supports_context_tracking(&self) -> bool {
        false
    }
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Codex => write!(f, "codex"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_type_from_command() {
        assert_eq!(
            AgentType::from_command(&["hirsel".to_string(), "__worker-run".to_string()]),
            AgentType::Codex
        );
        assert_eq!(
            AgentType::from_command(&["codex".to_string()]),
            AgentType::Codex
        );
        assert_eq!(AgentType::from_command(&[]), AgentType::Unknown);
    }
}
