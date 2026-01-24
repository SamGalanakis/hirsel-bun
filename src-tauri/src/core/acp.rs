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
//!     command: vec!["hirsel __acp-bridge".into()],
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
use tokio::process::{Child, ChildStdin, ChildStdout};
use tracing::info;

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
    /// The command to run the agent (e.g., ["hirsel __acp-bridge"]).
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
            command: vec!["hirsel".into(), "__acp-bridge".into()],
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

    let mut env = HashMap::from([
        ("HIRSEL_RUN".into(), run_name.into()),
        ("HIRSEL_WORKER".into(), worker_name.into()),
    ]);

    // Pass HIRSEL_API_URL to MCP server so it knows to use HttpState
    // for remote/Docker workers communicating with the daemon
    if let Ok(api_url) = std::env::var("HIRSEL_API_URL") {
        env.insert("HIRSEL_API_URL".into(), api_url);
    }

    // Pass HIRSEL_RUN_DIR so MCP server knows where the run directory is
    // (important for Docker where run_dir is mounted at a custom path like /hirsel)
    if let Ok(run_dir) = std::env::var("HIRSEL_RUN_DIR") {
        env.insert("HIRSEL_RUN_DIR".into(), run_dir);
    }

    MCPServerConfig::new("hirsel", hirsel, vec!["__worker-mcp".into()]).with_env(env)
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

// =============================================================================
// AcpChild - Reusable wrapper for ACP process spawning and cleanup
// =============================================================================

/// Configuration for spawning an ACP child process.
#[derive(Debug, Clone)]
pub struct AcpSpawnConfig {
    /// The command to run (e.g., ["hirsel __acp-bridge"]).
    pub command: Vec<String>,
    /// Working directory for the agent.
    pub cwd: PathBuf,
    /// Context name for logging (e.g., "worker-alpha", "eval").
    pub context: String,
    /// Whether to bypass permission prompts.
    pub bypass_permissions: bool,
    /// Additional environment variables beyond the standard agent env vars.
    pub extra_env: HashMap<String, String>,
}

impl AcpSpawnConfig {
    /// Create a new spawn config with default settings.
    pub fn new(command: Vec<String>, cwd: PathBuf, context: impl Into<String>) -> Self {
        Self {
            command,
            cwd,
            context: context.into(),
            bypass_permissions: true,
            extra_env: HashMap::new(),
        }
    }

    /// Set whether to bypass permission prompts (default: true).
    pub fn bypass_permissions(mut self, bypass: bool) -> Self {
        self.bypass_permissions = bypass;
        self
    }

    /// Add extra environment variables.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_env.insert(key.into(), value.into());
        self
    }
}

/// A wrapper around a spawned ACP child process that handles cleanup on drop.
///
/// This struct ensures that when the ACP process is dropped (either explicitly
/// or when it goes out of scope), the entire process group is cleaned up,
/// including any grandchild processes like `node hirsel __acp-bridge`.
///
/// # Example
///
/// ```rust,ignore
/// use hirsel_lib::core::acp::{AcpChild, AcpSpawnConfig};
///
/// let config = AcpSpawnConfig::new(
///     vec!["hirsel __acp-bridge".into()],
///     PathBuf::from("/project"),
///     "my-worker",
/// );
///
/// let mut child = AcpChild::spawn(config).await?;
/// let stdin = child.take_stdin().unwrap();
/// let stdout = child.take_stdout().unwrap();
///
/// // ... use stdin/stdout for ACP communication ...
///
/// // When `child` is dropped, the process group is automatically cleaned up
/// ```
pub struct AcpChild {
    child: Child,
    context: String,
    pid: Option<u32>,
}

impl AcpChild {
    /// Spawn a new ACP child process with the given configuration.
    ///
    /// The process is spawned with:
    /// - Its own process group (for proper cleanup of child processes)
    /// - stdin/stdout piped for ACP communication
    /// - stderr set to null
    /// - Standard agent environment variables (API keys, etc.)
    /// - Optional permission bypass
    /// - Any extra environment variables from the config
    pub fn spawn(config: AcpSpawnConfig) -> Result<Self> {
        use std::process::Stdio;
        use tokio::process::Command;

        if config.command.is_empty() {
            return Err(ACPError::Protocol("Empty command".into()));
        }

        // Resolve "hirsel" command to current executable path
        // This ensures the command works even when hirsel isn't in PATH
        let exe_path = if config.command[0] == "hirsel" {
            std::env::current_exe()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| config.command[0].clone())
        } else {
            config.command[0].clone()
        };

        let mut cmd = Command::new(&exe_path);
        if config.command.len() > 1 {
            cmd.args(&config.command[1..]);
        }
        cmd.current_dir(&config.cwd);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit()); // Inherit stderr so we can see errors

        // Create a new process group so we can kill all descendants on cleanup.
        // This is critical because hirsel __acp-bridge spawns Node.js processes that
        // may ignore SIGTERM, but SIGKILL to the process group will kill them.
        #[cfg(unix)]
        cmd.process_group(0);

        // Add standard agent environment variables (API keys, etc.)
        for (key, value) in collect_agent_env() {
            cmd.env(&key, &value);
        }

        // Add permission bypass if requested
        if config.bypass_permissions {
            cmd.env("ACP_PERMISSION_MODE", "bypassPermissions");
        }

        // Add any extra environment variables
        for (key, value) in &config.extra_env {
            cmd.env(key, value);
        }

        let child = cmd.spawn()?;
        let pid = child.id();

        info!(
            "[{}] ACP process spawned as process group leader, pid={}",
            config.context,
            pid.unwrap_or(0)
        );

        Ok(Self {
            child,
            context: config.context,
            pid,
        })
    }

    /// Take ownership of the child's stdin handle.
    ///
    /// This can only be called once - subsequent calls return None.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }

    /// Take ownership of the child's stdout handle.
    ///
    /// This can only be called once - subsequent calls return None.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    /// Get the process ID of the child process.
    pub fn id(&self) -> Option<u32> {
        self.pid
    }

    /// Get the context name for this process.
    pub fn context(&self) -> &str {
        &self.context
    }

    /// Wait for the child process to exit.
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    /// Attempt to kill the child process.
    pub async fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill().await
    }

    /// Start the child process if it hasn't been started yet.
    ///
    /// Returns a mutable reference to the underlying Child for advanced use cases.
    pub fn inner(&mut self) -> &mut Child {
        &mut self.child
    }

    /// Clean up the process group for this ACP child.
    ///
    /// This is called automatically on drop, but can also be called manually
    /// if you need to ensure cleanup happens at a specific point.
    pub fn cleanup(&self) {
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            info!(
                "[{}] Cleaning up ACP process group (PID {})",
                self.context, pid
            );
            unsafe {
                // Send SIGTERM first for graceful shutdown
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
            // Brief wait for graceful shutdown
            std::thread::sleep(std::time::Duration::from_millis(100));
            // Force kill - Node.js processes may ignore SIGTERM
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

impl Drop for AcpChild {
    fn drop(&mut self) {
        self.cleanup();
    }
}

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
        assert_eq!(config.command, vec!["hirsel", "__acp-bridge"]);
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
