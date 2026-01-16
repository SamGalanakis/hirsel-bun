//! Agent Control Protocol (ACP) client for hirsel.
//!
//! This module provides functionality to spawn and communicate with AI coding agents
//! (like Claude Code) using the Agent Client Protocol.
//!
//! ## Overview
//!
//! The ACP client:
//! - Spawns agent processes as subprocesses communicating over stdio
//! - Manages sessions for agent conversations
//! - Sends prompts and receives responses
//! - Handles permission requests (auto-approved)
//! - Provides file and terminal operations for the agent
//!
//! ## Example
//!
//! ```rust,ignore
//! use hirsel_lib::core::acp::{ACPClient, ACPClientConfig};
//! use std::path::PathBuf;
//!
//! let config = ACPClientConfig {
//!     command: vec!["claude-code-acp".into()],
//!     cwd: PathBuf::from("/path/to/project"),
//!     ..Default::default()
//! };
//!
//! let client = ACPClient::new(config);
//! // Use client.start(), client.new_session(), client.prompt(), etc.
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

/// Error type for ACP operations.
#[derive(Debug, Error)]
pub enum ACPError {
    #[error("Agent not started")]
    NotStarted,

    #[error("Agent process exited unexpectedly")]
    ProcessExited,

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Result type for ACP operations.
pub type Result<T> = std::result::Result<T, ACPError>;

/// A session update notification from the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUpdate {
    /// The session ID this update belongs to.
    pub session_id: String,
    /// The type of update (e.g., "agent_message_chunk", "tool_call_start").
    pub update_type: String,
    /// The update content as a JSON object.
    pub content: serde_json::Value,
}

/// Configuration for an MCP server that the agent can use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPServerConfig {
    /// Name of the MCP server.
    pub name: String,
    /// Command to run the MCP server.
    pub command: String,
    /// Arguments for the MCP server command.
    pub args: Vec<String>,
    /// Environment variables for the MCP server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<EnvVariable>>,
}

/// An environment variable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVariable {
    pub name: String,
    pub value: String,
}

impl MCPServerConfig {
    /// Create a new MCP server configuration.
    pub fn new(name: impl Into<String>, command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args,
            env: None,
        }
    }

    /// Add environment variables to the MCP server configuration.
    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = Some(
            env.into_iter()
                .map(|(name, value)| EnvVariable { name, value })
                .collect(),
        );
        self
    }
}

/// Configuration for the ACP client.
#[derive(Debug, Clone)]
pub struct ACPClientConfig {
    /// The command to run the agent (e.g., ["claude-code-acp"]).
    pub command: Vec<String>,
    /// Working directory for the agent.
    pub cwd: PathBuf,
    /// MCP servers to configure for the agent.
    pub mcp_servers: Vec<MCPServerConfig>,
    /// Environment variables to pass to the agent process.
    pub env: HashMap<String, String>,
}

impl Default for ACPClientConfig {
    fn default() -> Self {
        Self {
            command: vec!["claude-code-acp".into()],
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            mcp_servers: Vec::new(),
            env: HashMap::new(),
        }
    }
}

/// Metrics about an agent session.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionMetrics {
    /// Tokens used in input.
    pub input_tokens: u64,
    /// Tokens used in output.
    pub output_tokens: u64,
    /// Cache creation tokens.
    pub cache_creation_tokens: u64,
    /// Cache read tokens.
    pub cache_read_tokens: u64,
    /// Total cost in USD.
    pub total_cost_usd: f64,
    /// Context utilization (0.0 to 1.0).
    pub context_utilization: f64,
}

/// Result of initializing the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// Protocol version the agent supports.
    pub protocol_version: String,
    /// Agent implementation info.
    pub agent_info: Option<AgentInfo>,
}

/// Information about the agent implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Name of the agent.
    pub name: String,
    /// Version of the agent.
    pub version: String,
    /// Display title.
    pub title: Option<String>,
}

/// Result of creating a new session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSessionResult {
    /// The session ID.
    pub session_id: String,
}

/// Result of sending a prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptResult {
    /// Whether the prompt completed successfully.
    pub success: bool,
    /// Any error message.
    pub error: Option<String>,
}

/// Builder for creating hirsel's MCP server configuration.
///
/// This creates the configuration needed to expose hirsel's worker
/// commands (task_claim, task_done, msg_send, etc.) to the agent.
pub fn create_hirsel_mcp_config(
    run_name: &str,
    worker_name: &str,
    hirsel_path: Option<&str>,
) -> MCPServerConfig {
    let hirsel = hirsel_path.unwrap_or("hirsel");

    MCPServerConfig::new("hirsel", hirsel, vec!["__worker-mcp".into()]).with_env(HashMap::from([
        ("HIRSEL_RUN".into(), run_name.into()),
        ("HIRSEL_WORKER".into(), worker_name.into()),
    ]))
}

/// Environment variables that should be forwarded to agent processes.
///
/// These are API keys and configuration that agents need to function.
pub const AGENT_ENV_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "CLAUDE_ACCESS_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_MODEL",
    "MAX_THINKING_TOKENS",
    "GOOGLE_API_KEY",
    "OPENAI_API_KEY",
];

/// Collect environment variables that should be forwarded to agent processes.
pub fn collect_agent_env() -> HashMap<String, String> {
    let mut env = HashMap::new();
    for var in AGENT_ENV_VARS {
        if let Ok(value) = std::env::var(var) {
            env.insert(var.to_string(), value);
        }
    }
    env
}

/// Extract tool output from ACP ToolCallUpdateFields.
///
/// Tries raw_output first (JSON serialized), then falls back to extracting
/// text content from the content field.
pub fn extract_tool_output(fields: &agent_client_protocol::ToolCallUpdateFields) -> Option<String> {
    // Try raw_output first
    if let Some(ref raw) = fields.raw_output {
        if let Ok(s) = serde_json::to_string(raw) {
            return Some(s);
        }
    }

    // Fall back to extracting text from content
    fields.content.as_ref().and_then(|contents| {
        use agent_client_protocol::{ContentBlock, ToolCallContent};
        let texts: Vec<String> = contents
            .iter()
            .filter_map(|c| match c {
                ToolCallContent::Content(content) => match &content.content {
                    ContentBlock::Text(t) => Some(t.text.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        if texts.is_empty() {
            None
        } else {
            Some(texts.join("\n"))
        }
    })
}

// Note: The actual ACP client implementation requires async runtime support
// and the agent-client-protocol crate. The types above provide the interface
// that will be used by the worker system.
//
// The full implementation will:
// 1. Spawn the agent process with stdin/stdout piped
// 2. Use agent_client_protocol::ClientSideConnection for communication
// 3. Implement the Client trait to handle agent requests
// 4. Manage sessions and route prompts

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_server_config() {
        let config = MCPServerConfig::new("test", "test-cmd", vec!["arg1".into()]);
        assert_eq!(config.name, "test");
        assert_eq!(config.command, "test-cmd");
        assert_eq!(config.args, vec!["arg1"]);
        assert!(config.env.is_none());
    }

    #[test]
    fn test_mcp_server_config_with_env() {
        let config = MCPServerConfig::new("test", "cmd", vec![])
            .with_env(HashMap::from([("KEY".into(), "VALUE".into())]));

        let env = config.env.unwrap();
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].name, "KEY");
        assert_eq!(env[0].value, "VALUE");
    }

    #[test]
    fn test_create_hirsel_mcp_config() {
        let config = create_hirsel_mcp_config("my-run", "alpha", None);
        assert_eq!(config.name, "hirsel");
        assert_eq!(config.command, "hirsel");
        assert_eq!(config.args, vec!["__worker-mcp"]);

        let env = config.env.unwrap();
        assert!(env
            .iter()
            .any(|e| e.name == "HIRSEL_RUN" && e.value == "my-run"));
        assert!(env
            .iter()
            .any(|e| e.name == "HIRSEL_WORKER" && e.value == "alpha"));
    }

    #[test]
    fn test_acp_client_config_default() {
        let config = ACPClientConfig::default();
        assert_eq!(config.command, vec!["claude-code-acp"]);
        assert!(config.mcp_servers.is_empty());
        assert!(config.env.is_empty());
    }

    #[test]
    fn test_session_update_serialization() {
        let update = SessionUpdate {
            session_id: "sess_123".into(),
            update_type: "agent_message_chunk".into(),
            content: serde_json::json!({"text": "Hello"}),
        };

        let json = serde_json::to_string(&update).unwrap();
        let parsed: SessionUpdate = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.session_id, "sess_123");
        assert_eq!(parsed.update_type, "agent_message_chunk");
    }

    #[test]
    fn test_session_metrics_default() {
        let metrics = SessionMetrics::default();
        assert_eq!(metrics.input_tokens, 0);
        assert_eq!(metrics.output_tokens, 0);
        assert_eq!(metrics.context_utilization, 0.0);
    }

    #[test]
    fn test_collect_agent_env() {
        // This test just ensures the function doesn't panic
        let env = collect_agent_env();
        // The result depends on the actual environment
        assert!(env.len() <= AGENT_ENV_VARS.len());
    }
}
