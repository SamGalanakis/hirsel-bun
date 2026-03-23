//! Agent configuration.

use serde::{Deserialize, Serialize};

use super::types::AgentType;

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_agent_command")]
    pub command: Vec<String>,
}

fn default_agent_command() -> Vec<String> {
    // Single supported runtime path: embedded lash worker runtime.
    vec!["hirsel".to_string(), "__worker-runtime".to_string()]
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            command: default_agent_command(),
        }
    }
}

impl AgentConfig {
    /// Get the agent type from the command
    pub fn agent_type(&self) -> AgentType {
        AgentType::from_command(&self.command)
    }

    /// Whether this agent supports context tracking
    pub fn supports_context_tracking(&self) -> bool {
        self.agent_type().supports_context_tracking()
    }
}
