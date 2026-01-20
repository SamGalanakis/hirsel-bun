//! Claude CLI bridge for direct communication with Claude Code.
//!
//! This module provides a native Rust implementation for communicating with
//! the Claude CLI using its JSON streaming protocol.
//!
//! ## Protocol
//!
//! Claude CLI supports bidirectional JSON streaming:
//! ```bash
//! claude --input-format stream-json --output-format stream-json
//! ```
//!
//! Messages are JSON-lines (one JSON object per line).
//!
//! ## Example
//!
//! ```rust,ignore
//! use hirsel_lib::core::claude_cli::{ClaudeCliBridge, ClaudeCliConfig, BridgeEvent};
//!
//! let config = ClaudeCliConfig::new(PathBuf::from("/project"), "my-worker");
//! let (mut bridge, mut events) = ClaudeCliBridge::spawn(config)?;
//!
//! bridge.send_prompt("Fix the bug in main.rs").await?;
//!
//! while let Some(event) = events.recv().await {
//!     match event {
//!         BridgeEvent::TextDelta { text } => print!("{}", text),
//!         BridgeEvent::PermissionRequest { request_id, .. } => {
//!             bridge.respond_permission(&request_id, true).await?;
//!         }
//!         BridgeEvent::MessageComplete { .. } => break,
//!         _ => {}
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::acp::{collect_agent_env, MCPServerConfig};

// =============================================================================
// Error Types
// =============================================================================

/// Error type for Claude CLI bridge operations.
#[derive(Debug, Error)]
pub enum ClaudeCliError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Process not running")]
    NotRunning,
}

/// Result type for Claude CLI operations.
pub type Result<T> = std::result::Result<T, ClaudeCliError>;

// =============================================================================
// Input Message Types (sent TO the CLI)
// =============================================================================

/// Message sent to Claude CLI via stdin.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CliInputMessage {
    /// Send a user message/prompt
    User {
        message: UserMessage,
        session_id: String,
    },
    /// Response to a control request (permission check)
    ControlResponse {
        request_id: String,
        response: ControlResponsePayload,
    },
}

/// User message content.
#[derive(Debug, Clone, Serialize)]
pub struct UserMessage {
    pub role: String,
    pub content: UserContent,
}

/// User message content - can be string or structured.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum UserContent {
    Text(String),
    Blocks(Vec<UserContentBlock>),
}

/// A content block in a user message.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserContentBlock {
    Text { text: String },
}

/// Payload for a control response.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum ControlResponsePayload {
    Success { response: serde_json::Value },
    Error { error: String },
}

// =============================================================================
// Output Message Types (received FROM the CLI)
// =============================================================================

/// Message received from Claude CLI via stdout.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CliOutputMessage {
    /// Permission/tool check request from the agent
    ControlRequest {
        request_id: String,
        request: ControlRequest,
    },
    /// Assistant response (complete message)
    Assistant(AssistantMessage),
    /// Stream event (partial content)
    StreamEvent(StreamEvent),
    /// Final result with metrics
    Result(ResultMessage),
    /// System message
    System(SystemMessage),
    /// User message echo (when --replay-user-messages is used)
    User(serde_json::Value),
    /// Error from CLI
    Error { message: String },
}

/// Control request from the agent (permission check).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum ControlRequest {
    /// Built-in tool permission check
    CanUseTool {
        tool_name: String,
        input: serde_json::Value,
    },
    /// MCP tool permission check
    CanUseMcpTool {
        server_name: String,
        tool_name: String,
        input: serde_json::Value,
    },
    /// Hook callback
    HookCallback {
        callback_id: String,
        hook_type: String,
        #[serde(default)]
        data: serde_json::Value,
    },
}

/// Complete assistant message.
#[derive(Debug, Clone, Deserialize)]
pub struct AssistantMessage {
    pub message: MessageContent,
}

/// Message content with role and content blocks.
#[derive(Debug, Clone, Deserialize)]
pub struct MessageContent {
    pub role: String,
    #[serde(default)]
    pub content: Vec<CliContentBlock>,
}

/// A content block in a CLI message (distinct from agent_client_protocol::ContentBlock).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CliContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: serde_json::Value,
    },
    Thinking {
        thinking: String,
    },
}

/// Stream event for partial content.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Content block started
    ContentBlockStart {
        index: u32,
        content_block: ContentBlockStart,
    },
    /// Content block delta (partial content)
    ContentBlockDelta { index: u32, delta: ContentDelta },
    /// Content block completed
    ContentBlockStop { index: u32 },
    /// Message started
    MessageStart {
        #[serde(default)]
        message: serde_json::Value,
    },
    /// Message delta
    MessageDelta {
        #[serde(default)]
        delta: serde_json::Value,
        #[serde(default)]
        usage: serde_json::Value,
    },
    /// Message completed
    MessageStop,
}

/// Start of a content block.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlockStart {
    Text {
        #[serde(default)]
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
    },
    Thinking {
        #[serde(default)]
        thinking: String,
    },
}

/// Delta (partial content) for a content block.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
    ThinkingDelta { thinking: String },
}

/// Final result message with metrics.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResultMessage {
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub duration_api_ms: Option<u64>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub subtype: Option<String>,
}

/// System message from the CLI.
#[derive(Debug, Clone, Deserialize)]
pub struct SystemMessage {
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

// =============================================================================
// Permission Response Types
// =============================================================================

/// Response to a tool permission request.
#[derive(Debug, Clone, Serialize)]
pub struct ToolPermissionResponse {
    pub behavior: ToolPermissionBehavior,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Value>,
}

/// Permission behavior options.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolPermissionBehavior {
    Allow,
    Deny,
    AllowAlways,
}

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for spawning Claude CLI.
#[derive(Debug, Clone)]
pub struct ClaudeCliConfig {
    /// Working directory for the CLI.
    pub cwd: PathBuf,
    /// MCP servers to configure.
    pub mcp_servers: Vec<MCPServerConfig>,
    /// Additional environment variables.
    pub env: HashMap<String, String>,
    /// Context name for logging.
    pub context: String,
    /// Whether to auto-approve all permission requests.
    pub bypass_permissions: bool,
    /// Model to use (optional, uses CLI default if not set).
    pub model: Option<String>,
    /// System prompt (optional).
    pub system_prompt: Option<String>,
    /// Path to claude binary (optional, uses "claude" from PATH if not set).
    pub claude_path: Option<PathBuf>,
    /// Additional CLI arguments.
    pub extra_args: Vec<String>,
}

impl ClaudeCliConfig {
    /// Create a new config with defaults.
    pub fn new(cwd: PathBuf, context: impl Into<String>) -> Self {
        Self {
            cwd,
            mcp_servers: Vec::new(),
            env: HashMap::new(),
            context: context.into(),
            bypass_permissions: true,
            model: None,
            system_prompt: None,
            claude_path: None,
            extra_args: Vec::new(),
        }
    }

    /// Add MCP servers.
    pub fn with_mcp_servers(mut self, servers: Vec<MCPServerConfig>) -> Self {
        self.mcp_servers = servers;
        self
    }

    /// Add an environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set whether to bypass permissions (auto-approve all).
    pub fn bypass_permissions(mut self, bypass: bool) -> Self {
        self.bypass_permissions = bypass;
        self
    }

    /// Set the model to use.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the system prompt.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Set the path to the claude binary.
    pub fn with_claude_path(mut self, path: PathBuf) -> Self {
        self.claude_path = Some(path);
        self
    }

    /// Add extra CLI arguments.
    pub fn with_extra_args(mut self, args: Vec<String>) -> Self {
        self.extra_args = args;
        self
    }
}

// =============================================================================
// Bridge Events (translated output for consumers)
// =============================================================================

/// Events emitted by the bridge, translated from CLI output.
///
/// These events provide a simplified view of the CLI's JSON output,
/// suitable for consumption by hirsel's worker and chat systems.
#[derive(Debug, Clone)]
pub enum BridgeEvent {
    /// Session initialized with ID.
    SessionInit { session_id: String },

    /// Text chunk from the assistant.
    TextDelta { text: String },

    /// Thinking/reasoning chunk from the assistant.
    ThinkingDelta { thinking: String },

    /// Tool call started.
    ToolCallStart {
        tool_call_id: String,
        tool_name: String,
        input: serde_json::Value,
    },

    /// Tool call input being streamed (partial JSON).
    ToolCallInputDelta {
        tool_call_id: String,
        partial_json: String,
    },

    /// Tool call completed.
    ToolCallComplete {
        tool_call_id: String,
        output: Option<String>,
    },

    /// Permission request from the agent.
    /// If bypass_permissions is true, these are auto-approved internally.
    /// Otherwise, the consumer must call respond_permission().
    PermissionRequest {
        request_id: String,
        tool_name: String,
        server_name: Option<String>,
        input: serde_json::Value,
    },

    /// Hook callback request.
    HookCallback {
        request_id: String,
        callback_id: String,
        hook_type: String,
        data: serde_json::Value,
    },

    /// Message completed with metrics.
    MessageComplete {
        stop_reason: Option<String>,
        result: Option<String>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        cost_usd: Option<f64>,
        duration_ms: Option<u64>,
    },

    /// Error from the CLI.
    Error { message: String },

    /// CLI process exited.
    ProcessExited { code: Option<i32> },
}

// =============================================================================
// Claude CLI Bridge
// =============================================================================

/// Bridge for communicating with Claude CLI.
///
/// This struct manages the CLI process lifecycle and provides methods for
/// sending prompts and handling permission requests.
pub struct ClaudeCliBridge {
    child: Child,
    stdin: ChildStdin,
    context: String,
    pid: Option<u32>,
    bypass_permissions: bool,
    session_id: String,
}

impl ClaudeCliBridge {
    /// Spawn a new Claude CLI process with JSON streaming.
    ///
    /// Returns the bridge and a receiver for events.
    pub fn spawn(config: ClaudeCliConfig) -> Result<(Self, mpsc::UnboundedReceiver<BridgeEvent>)> {
        use std::process::Stdio;
        use tokio::process::Command;

        let claude_bin = config
            .claude_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "claude".to_string());

        let mut cmd = Command::new(&claude_bin);

        // Required flags for JSON streaming
        cmd.arg("--input-format").arg("stream-json");
        cmd.arg("--output-format").arg("stream-json");
        cmd.arg("--verbose"); // Required when using --output-format=stream-json

        // Use delegate permission mode - this sends control_request messages to us
        // so we can approve/deny tools. Other modes either auto-approve or block internally.
        cmd.arg("--permission-mode").arg("delegate");

        // Set working directory
        cmd.current_dir(&config.cwd);

        // Add model if specified
        if let Some(ref model) = config.model {
            cmd.arg("--model").arg(model);
        }

        // Add system prompt if specified
        if let Some(ref prompt) = config.system_prompt {
            cmd.arg("--system-prompt").arg(prompt);
        }

        // Add MCP servers
        // Format: --mcp-config '{"mcpServers":{"name":{"command":"cmd","args":["a1"],"env":{"K":"V"}}}}'
        if !config.mcp_servers.is_empty() {
            let mut mcp_servers = serde_json::Map::new();
            for mcp in &config.mcp_servers {
                let mut server_config = serde_json::json!({
                    "command": mcp.command,
                    "args": mcp.args,
                });
                if let Some(ref env_vars) = mcp.env {
                    let env_map: HashMap<String, String> = env_vars
                        .iter()
                        .map(|v| (v.name.clone(), v.value.clone()))
                        .collect();
                    server_config["env"] = serde_json::to_value(env_map).unwrap_or_default();
                }
                mcp_servers.insert(mcp.name.clone(), server_config);
            }
            let mcp_config = serde_json::json!({ "mcpServers": mcp_servers });
            cmd.arg("--mcp-config").arg(mcp_config.to_string());
        }

        // Add extra arguments
        for arg in &config.extra_args {
            cmd.arg(arg);
        }

        // Stdio configuration
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit()); // Let stderr pass through for debugging

        // Process group for cleanup
        #[cfg(unix)]
        cmd.process_group(0);

        // Add standard agent environment variables (API keys, etc.)
        for (key, value) in collect_agent_env() {
            cmd.env(&key, &value);
        }

        // Add custom environment variables
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        // IMPORTANT: Remove ACP_PERMISSION_MODE to ensure Claude CLI doesn't auto-approve permissions.
        // The worker subprocess has this set to "bypassPermissions" but we want to handle
        // permission requests ourselves via the bridge so we can auto-approve MCP tools.
        cmd.env_remove("ACP_PERMISSION_MODE");

        // Spawn the process
        let mut child = cmd.spawn().map_err(|e| {
            ClaudeCliError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to spawn claude CLI '{}': {}", claude_bin, e),
            ))
        })?;

        let pid = child.id();

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ClaudeCliError::Protocol("Failed to get stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ClaudeCliError::Protocol("Failed to get stdout".into()))?;

        info!(
            "[{}] Claude CLI spawned, pid={}, cwd={}",
            config.context,
            pid.unwrap_or(0),
            config.cwd.display()
        );

        // Create event channel
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Spawn output reader task
        let context = config.context.clone();
        let bypass = config.bypass_permissions;
        tokio::spawn(Self::read_output_loop(
            stdout,
            event_tx,
            context.clone(),
            bypass,
        ));

        let bridge = Self {
            child,
            stdin,
            context: config.context,
            pid,
            bypass_permissions: config.bypass_permissions,
            session_id: "default".to_string(),
        };

        Ok((bridge, event_rx))
    }

    /// Send a user message/prompt to the CLI.
    pub async fn send_prompt(&mut self, content: &str) -> Result<()> {
        let msg = CliInputMessage::User {
            message: UserMessage {
                role: "user".to_string(),
                content: UserContent::Text(content.to_string()),
            },
            session_id: self.session_id.clone(),
        };
        self.send_message(&msg).await
    }

    /// Send a user message with multiple content blocks.
    pub async fn send_prompt_blocks(&mut self, blocks: Vec<UserContentBlock>) -> Result<()> {
        let msg = CliInputMessage::User {
            message: UserMessage {
                role: "user".to_string(),
                content: UserContent::Blocks(blocks),
            },
            session_id: self.session_id.clone(),
        };
        self.send_message(&msg).await
    }

    /// Respond to a permission request.
    pub async fn respond_permission(&mut self, request_id: &str, allow: bool) -> Result<()> {
        let behavior = if allow {
            ToolPermissionBehavior::Allow
        } else {
            ToolPermissionBehavior::Deny
        };

        let msg = CliInputMessage::ControlResponse {
            request_id: request_id.to_string(),
            response: ControlResponsePayload::Success {
                response: serde_json::to_value(ToolPermissionResponse {
                    behavior,
                    updated_input: None,
                })
                .unwrap(),
            },
        };
        self.send_message(&msg).await
    }

    /// Respond to a permission request with "allow always" (remember for session).
    pub async fn respond_permission_always(&mut self, request_id: &str) -> Result<()> {
        let msg = CliInputMessage::ControlResponse {
            request_id: request_id.to_string(),
            response: ControlResponsePayload::Success {
                response: serde_json::to_value(ToolPermissionResponse {
                    behavior: ToolPermissionBehavior::AllowAlways,
                    updated_input: None,
                })
                .unwrap(),
            },
        };
        self.send_message(&msg).await
    }

    /// Respond to a hook callback.
    pub async fn respond_hook(
        &mut self,
        request_id: &str,
        response: serde_json::Value,
    ) -> Result<()> {
        let msg = CliInputMessage::ControlResponse {
            request_id: request_id.to_string(),
            response: ControlResponsePayload::Success { response },
        };
        self.send_message(&msg).await
    }

    /// Send a raw message to the CLI.
    async fn send_message(&mut self, msg: &CliInputMessage) -> Result<()> {
        let json = serde_json::to_string(msg)?;
        debug!("[{}] Sending: {}", self.context, json);
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    /// Read and process output from the CLI.
    async fn read_output_loop(
        stdout: ChildStdout,
        event_tx: mpsc::UnboundedSender<BridgeEvent>,
        context: String,
        bypass_permissions: bool,
    ) {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        // Track current tool calls by index for completion
        let mut active_tools: HashMap<u32, (String, String)> = HashMap::new(); // index -> (id, name)

        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }

            debug!("[{}] Received: {}", context, line);

            match serde_json::from_str::<CliOutputMessage>(&line) {
                Ok(msg) => {
                    Self::process_message(
                        msg,
                        &event_tx,
                        &context,
                        bypass_permissions,
                        &mut active_tools,
                    );
                }
                Err(e) => {
                    warn!("[{}] Failed to parse CLI output: {} - {}", context, e, line);
                }
            }
        }

        info!("[{}] Output reader loop ended", context);
        let _ = event_tx.send(BridgeEvent::ProcessExited { code: None });
    }

    /// Process a single message from the CLI.
    fn process_message(
        msg: CliOutputMessage,
        event_tx: &mpsc::UnboundedSender<BridgeEvent>,
        context: &str,
        bypass_permissions: bool,
        active_tools: &mut HashMap<u32, (String, String)>,
    ) {
        match msg {
            CliOutputMessage::ControlRequest {
                request_id,
                request,
            } => match request {
                ControlRequest::CanUseTool { tool_name, input } => {
                    if bypass_permissions {
                        debug!(
                            "[{}] Auto-approve needed for tool: {} (request {})",
                            context, tool_name, request_id
                        );
                    }
                    let _ = event_tx.send(BridgeEvent::PermissionRequest {
                        request_id,
                        tool_name,
                        server_name: None,
                        input,
                    });
                }
                ControlRequest::CanUseMcpTool {
                    server_name,
                    tool_name,
                    input,
                } => {
                    if bypass_permissions {
                        debug!(
                            "[{}] Auto-approve needed for MCP tool: {}:{} (request {})",
                            context, server_name, tool_name, request_id
                        );
                    }
                    let _ = event_tx.send(BridgeEvent::PermissionRequest {
                        request_id,
                        tool_name,
                        server_name: Some(server_name),
                        input,
                    });
                }
                ControlRequest::HookCallback {
                    callback_id,
                    hook_type,
                    data,
                } => {
                    let _ = event_tx.send(BridgeEvent::HookCallback {
                        request_id,
                        callback_id,
                        hook_type,
                        data,
                    });
                }
            },
            CliOutputMessage::StreamEvent(event) => {
                Self::process_stream_event(event, event_tx, active_tools);
            }
            CliOutputMessage::Result(result) => {
                let _ = event_tx.send(BridgeEvent::MessageComplete {
                    stop_reason: result.stop_reason,
                    result: result.result,
                    input_tokens: result.input_tokens,
                    output_tokens: result.output_tokens,
                    cost_usd: result.cost_usd,
                    duration_ms: result.duration_ms,
                });
            }
            CliOutputMessage::Error { message } => {
                let _ = event_tx.send(BridgeEvent::Error { message });
            }
            CliOutputMessage::System(sys) => {
                // Check for session init
                if let Some(session_id) = sys.session_id {
                    let _ = event_tx.send(BridgeEvent::SessionInit { session_id });
                }
            }
            CliOutputMessage::Assistant(msg) => {
                // Process complete message content blocks
                for block in msg.message.content {
                    match block {
                        CliContentBlock::Text { text } => {
                            if !text.is_empty() {
                                let _ = event_tx.send(BridgeEvent::TextDelta { text });
                            }
                        }
                        CliContentBlock::Thinking { thinking } => {
                            if !thinking.is_empty() {
                                let _ = event_tx.send(BridgeEvent::ThinkingDelta { thinking });
                            }
                        }
                        CliContentBlock::ToolUse { id, name, input } => {
                            let _ = event_tx.send(BridgeEvent::ToolCallStart {
                                tool_call_id: id.clone(),
                                tool_name: name,
                                input,
                            });
                            let _ = event_tx.send(BridgeEvent::ToolCallComplete {
                                tool_call_id: id,
                                output: None,
                            });
                        }
                        CliContentBlock::ToolResult { .. } => {
                            // Tool results are typically handled separately
                        }
                    }
                }
            }
            CliOutputMessage::User(_) => {
                // Echo of user message, can ignore
            }
        }
    }

    /// Process a stream event.
    fn process_stream_event(
        event: StreamEvent,
        event_tx: &mpsc::UnboundedSender<BridgeEvent>,
        active_tools: &mut HashMap<u32, (String, String)>,
    ) {
        match event {
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => match content_block {
                ContentBlockStart::Text { text } => {
                    if !text.is_empty() {
                        let _ = event_tx.send(BridgeEvent::TextDelta { text });
                    }
                }
                ContentBlockStart::ToolUse { id, name } => {
                    active_tools.insert(index, (id.clone(), name.clone()));
                    let _ = event_tx.send(BridgeEvent::ToolCallStart {
                        tool_call_id: id,
                        tool_name: name,
                        input: serde_json::Value::Null,
                    });
                }
                ContentBlockStart::Thinking { thinking } => {
                    if !thinking.is_empty() {
                        let _ = event_tx.send(BridgeEvent::ThinkingDelta { thinking });
                    }
                }
            },
            StreamEvent::ContentBlockDelta { index, delta } => match delta {
                ContentDelta::TextDelta { text } => {
                    let _ = event_tx.send(BridgeEvent::TextDelta { text });
                }
                ContentDelta::ThinkingDelta { thinking } => {
                    let _ = event_tx.send(BridgeEvent::ThinkingDelta { thinking });
                }
                ContentDelta::InputJsonDelta { partial_json } => {
                    if let Some((id, _)) = active_tools.get(&index) {
                        let _ = event_tx.send(BridgeEvent::ToolCallInputDelta {
                            tool_call_id: id.clone(),
                            partial_json,
                        });
                    }
                }
            },
            StreamEvent::ContentBlockStop { index } => {
                if let Some((id, _name)) = active_tools.remove(&index) {
                    let _ = event_tx.send(BridgeEvent::ToolCallComplete {
                        tool_call_id: id,
                        output: None,
                    });
                }
            }
            StreamEvent::MessageStart { .. } | StreamEvent::MessageDelta { .. } => {
                // These contain metadata we don't need to forward
            }
            StreamEvent::MessageStop => {
                // Message completed, Result message should follow
            }
        }
    }

    /// Get the process ID.
    pub fn id(&self) -> Option<u32> {
        self.pid
    }

    /// Get the context name.
    pub fn context(&self) -> &str {
        &self.context
    }

    /// Check if bypass_permissions is enabled.
    pub fn is_bypass_permissions(&self) -> bool {
        self.bypass_permissions
    }

    /// Clean up the process group.
    pub fn cleanup(&self) {
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            info!(
                "[{}] Cleaning up Claude CLI process group (PID {})",
                self.context, pid
            );
            unsafe {
                // Send SIGTERM first for graceful shutdown
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
            // Brief wait for graceful shutdown
            std::thread::sleep(std::time::Duration::from_millis(100));
            // Force kill
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }

    /// Wait for the process to exit.
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }
}

impl Drop for ClaudeCliBridge {
    fn drop(&mut self) {
        self.cleanup();
    }
}

// =============================================================================
// Worker Runner - Using ACP SessionUpdate Interface
// =============================================================================

// Note: create_hirsel_mcp_config no longer used - SDK handles MCP config directly
use agent_client_protocol::{
    Client as AcpClient, ContentBlock as AcpContentBlock, ContentChunk, SessionNotification,
    SessionUpdate, TextContent, ToolCall as AcpToolCall, ToolCallId,
    ToolCallStatus as AcpToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use std::sync::Arc;

/// Configuration for running a Claude worker.
#[derive(Debug, Clone)]
pub struct ClaudeWorkerConfig {
    pub run_name: String,
    pub worker_name: String,
    pub work_dir: PathBuf,
    pub run_dir: PathBuf,
    pub prompt: String,
}

/// Result of a worker run.
#[derive(Debug, Default)]
pub struct WorkerResult {
    pub stop_reason: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
}

/// Convert a BridgeEvent to an ACP SessionUpdate.
///
/// This bridges the Claude CLI JSON protocol to the standard ACP interface,
/// allowing the same HirselClient to process events from both backends.
/// Note: This is kept for the legacy ClaudeCliBridge approach.
#[allow(dead_code)]
fn bridge_event_to_session_update(
    event: &BridgeEvent,
    active_tools: &HashMap<String, String>,
) -> Option<SessionUpdate> {
    match event {
        BridgeEvent::TextDelta { text } => Some(SessionUpdate::AgentMessageChunk(
            ContentChunk::new(AcpContentBlock::Text(TextContent::new(text.clone()))),
        )),
        BridgeEvent::ThinkingDelta { thinking } => Some(SessionUpdate::AgentThoughtChunk(
            ContentChunk::new(AcpContentBlock::Text(TextContent::new(thinking.clone()))),
        )),
        BridgeEvent::ToolCallStart {
            tool_call_id,
            tool_name,
            input,
        } => {
            let kind = tool_name_to_acp_kind(tool_name);
            let raw_input = if input.is_null() {
                None
            } else {
                Some(input.clone())
            };

            Some(SessionUpdate::ToolCall(
                AcpToolCall::new(ToolCallId::new(tool_call_id.clone()), tool_name.clone())
                    .kind(kind)
                    .status(AcpToolCallStatus::InProgress)
                    .raw_input(raw_input),
            ))
        }
        BridgeEvent::ToolCallComplete {
            tool_call_id,
            output,
        } => {
            let mut fields = ToolCallUpdateFields::new().status(AcpToolCallStatus::Completed);
            if let Some(out) = output {
                fields = fields.raw_output(serde_json::Value::String(out.clone()));
            }

            Some(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                ToolCallId::new(tool_call_id.clone()),
                fields,
            )))
        }
        BridgeEvent::ToolCallInputDelta { tool_call_id, .. } => {
            // Update tool with partial input - use title field to show progress
            let title = active_tools
                .get(tool_call_id)
                .map(|name| format!("{} (streaming input...)", name));

            let mut fields = ToolCallUpdateFields::new();
            if let Some(t) = title {
                fields = fields.title(t);
            }

            Some(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                ToolCallId::new(tool_call_id.clone()),
                fields,
            )))
        }
        // These events don't map to SessionUpdate
        BridgeEvent::SessionInit { .. }
        | BridgeEvent::PermissionRequest { .. }
        | BridgeEvent::HookCallback { .. }
        | BridgeEvent::MessageComplete { .. }
        | BridgeEvent::Error { .. }
        | BridgeEvent::ProcessExited { .. } => None,
    }
}

/// Map tool name to ACP ToolKind.
fn tool_name_to_acp_kind(tool_name: &str) -> ToolKind {
    match tool_name.to_lowercase().as_str() {
        "read" | "readtextfile" | "read_text_file" => ToolKind::Read,
        "edit" | "write" | "writetextfile" | "write_text_file" => ToolKind::Edit,
        "bash" | "terminal" | "execute" | "createterminal" => ToolKind::Execute,
        "glob" | "grep" | "search" => ToolKind::Search,
        "webfetch" | "websearch" | "fetch" => ToolKind::Fetch,
        "task" => ToolKind::Think,
        _ => ToolKind::Other,
    }
}

/// Run a Claude worker using the Anthropic Claude Agent SDK.
///
/// This uses the official SDK which handles permissions properly and provides
/// a clean async interface for interacting with Claude.
#[cfg(feature = "claude")]
pub async fn run_claude_worker(config: ClaudeWorkerConfig) -> Result<WorkerResult> {
    use crate::worker::acp_client::HirselClient;
    use claude_agent_sdk::{
        query, ClaudeAgentOptions, McpServerConfig, McpStdioServerConfig, Message,
        PermissionManager, PermissionResult, PermissionResultAllow,
    };
    use futures::StreamExt;

    info!(
        "[{}] Starting Claude worker (SDK) for run={}",
        config.worker_name, config.run_name
    );

    // Create the HirselClient for processing SessionUpdate events
    let db_path = config.run_dir.join("hirsel.db");
    let client = Arc::new(HirselClient::new(&config.worker_name, &db_path));

    // Get the path to the current hirsel binary for MCP server
    let hirsel_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "hirsel".to_string());

    // Build MCP server configuration for hirsel
    let mut mcp_servers = std::collections::HashMap::new();
    let mut env = std::collections::HashMap::new();
    env.insert("HIRSEL_RUN".to_string(), config.run_name.clone());
    env.insert("HIRSEL_WORKER".to_string(), config.worker_name.clone());

    mcp_servers.insert(
        "hirsel".to_string(),
        McpServerConfig::Stdio(McpStdioServerConfig {
            server_type: Some("stdio".to_string()),
            command: hirsel_path,
            args: Some(vec!["__worker-mcp".to_string()]),
            env: Some(env),
        }),
    );

    // Create permission callback that auto-approves all tools
    let worker_name = config.worker_name.clone();
    let permission_callback =
        PermissionManager::callback(move |tool_name, _tool_input, _context| {
            let worker = worker_name.clone();
            async move {
                debug!("[{}] Auto-approving tool: {}", worker, tool_name.as_str());
                Ok(PermissionResult::Allow(PermissionResultAllow {
                    updated_input: None,
                    updated_permissions: None,
                }))
            }
        });

    // Build Claude options
    let options = ClaudeAgentOptions::builder()
        .mcp_servers(mcp_servers)
        .can_use_tool(permission_callback)
        .cwd(config.work_dir.clone())
        .build();

    info!(
        "[{}] Sending prompt ({} chars)",
        config.worker_name,
        config.prompt.len()
    );

    // Start the query
    let stream = query(&config.prompt, Some(options)).await.map_err(|e| {
        ClaudeCliError::Protocol(format!("Failed to start Claude SDK query: {}", e))
    })?;
    let mut stream = Box::pin(stream);

    // Process events using the ACP interface
    let mut result = WorkerResult::default();
    let mut active_tool_names: HashMap<String, String> = HashMap::new();
    let session_id = format!("claude-sdk-{}", config.worker_name);

    while let Some(message_result) = stream.next().await {
        let message = match message_result {
            Ok(m) => m,
            Err(e) => {
                warn!("[{}] Error from SDK: {}", config.worker_name, e);
                continue;
            }
        };

        match message {
            Message::Assistant { message, .. } => {
                // Process content blocks from assistant message
                for block in message.content {
                    match block {
                        claude_agent_sdk::ContentBlock::Text { text } => {
                            if !text.is_empty() {
                                let update = SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                    AcpContentBlock::Text(TextContent::new(text)),
                                ));
                                let notification =
                                    SessionNotification::new(session_id.clone(), update);
                                let _ = client.session_notification(notification).await;
                            }
                        }
                        claude_agent_sdk::ContentBlock::Thinking { thinking, .. } => {
                            if !thinking.is_empty() {
                                let update = SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                                    AcpContentBlock::Text(TextContent::new(thinking)),
                                ));
                                let notification =
                                    SessionNotification::new(session_id.clone(), update);
                                let _ = client.session_notification(notification).await;
                            }
                        }
                        claude_agent_sdk::ContentBlock::ToolUse { id, name, input } => {
                            active_tool_names.insert(id.clone(), name.clone());
                            let kind = tool_name_to_acp_kind(&name);
                            let update = SessionUpdate::ToolCall(
                                AcpToolCall::new(ToolCallId::new(id), name)
                                    .kind(kind)
                                    .status(AcpToolCallStatus::InProgress)
                                    .raw_input(Some(input)),
                            );
                            let notification = SessionNotification::new(session_id.clone(), update);
                            let _ = client.session_notification(notification).await;
                        }
                        claude_agent_sdk::ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } => {
                            active_tool_names.remove(&tool_use_id);
                            let output = content.and_then(|c| match c {
                                claude_agent_sdk::ContentValue::String(s) => Some(s),
                                claude_agent_sdk::ContentValue::Blocks(_) => None,
                            });
                            let mut fields =
                                ToolCallUpdateFields::new().status(AcpToolCallStatus::Completed);
                            if let Some(out) = output {
                                fields = fields.raw_output(serde_json::Value::String(out));
                            }
                            let update = SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                                ToolCallId::new(tool_use_id),
                                fields,
                            ));
                            let notification = SessionNotification::new(session_id.clone(), update);
                            let _ = client.session_notification(notification).await;
                        }
                    }
                }
            }
            Message::Result {
                total_cost_usd,
                duration_ms,
                is_error,
                result: result_msg,
                ..
            } => {
                info!(
                    "[{}] Session complete: is_error={}, cost=${:?}",
                    config.worker_name, is_error, total_cost_usd
                );
                result = WorkerResult {
                    stop_reason: if is_error {
                        Some("error".to_string())
                    } else {
                        Some("end_turn".to_string())
                    },
                    input_tokens: None, // SDK doesn't expose this in Result
                    output_tokens: None,
                    cost_usd: total_cost_usd,
                    duration_ms: Some(duration_ms),
                };
                if let Some(msg) = result_msg {
                    debug!("[{}] Result message: {}", config.worker_name, msg);
                }
            }
            _ => {}
        }
    }

    info!("[{}] Worker finished", config.worker_name);

    Ok(result)
}

/// Fallback implementation when claude feature is not enabled.
#[cfg(not(feature = "claude"))]
pub async fn run_claude_worker(_config: ClaudeWorkerConfig) -> Result<WorkerResult> {
    Err(ClaudeCliError::Protocol(
        "Claude SDK feature not enabled. Build with --features claude".into(),
    ))
}

/// Execute a Claude worker in a LocalSet context.
///
/// This is the entry point for spawning workers - it sets up the tokio runtime
/// and LocalSet required for the async operations.
pub fn execute_claude_worker(config: ClaudeWorkerConfig) -> Result<WorkerResult> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| ClaudeCliError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

    rt.block_on(async {
        tokio::task::LocalSet::new()
            .run_until(run_claude_worker(config))
            .await
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_message_serialization() {
        let msg = CliInputMessage::User {
            message: UserMessage {
                role: "user".to_string(),
                content: UserContent::Text("Hello".to_string()),
            },
            session_id: "default".to_string(),
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"user\""));
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"Hello\""));
        assert!(json.contains("\"session_id\":\"default\""));
    }

    #[test]
    fn test_control_response_serialization() {
        let msg = CliInputMessage::ControlResponse {
            request_id: "req_123".to_string(),
            response: ControlResponsePayload::Success {
                response: serde_json::to_value(ToolPermissionResponse {
                    behavior: ToolPermissionBehavior::Allow,
                    updated_input: None,
                })
                .unwrap(),
            },
        };

        let json = serde_json::to_string(&msg).unwrap();
        println!("Control response JSON: {}", json);
        assert!(json.contains("\"type\":\"control_response\""));
        assert!(json.contains("\"request_id\":\"req_123\""));
        assert!(json.contains("\"behavior\":\"allow\""));
    }

    #[test]
    fn test_parse_control_request() {
        let json = r#"{"type":"control_request","request_id":"req_1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#;
        let msg: CliOutputMessage = serde_json::from_str(json).unwrap();

        match msg {
            CliOutputMessage::ControlRequest {
                request_id,
                request,
            } => {
                assert_eq!(request_id, "req_1");
                match request {
                    ControlRequest::CanUseTool { tool_name, .. } => {
                        assert_eq!(tool_name, "Bash");
                    }
                    _ => panic!("Wrong request type"),
                }
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_parse_stream_event_text_delta() {
        let json = r#"{"type":"stream_event","event":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let msg: CliOutputMessage = serde_json::from_str(json).unwrap();

        match msg {
            CliOutputMessage::StreamEvent(StreamEvent::ContentBlockDelta { index, delta }) => {
                assert_eq!(index, 0);
                match delta {
                    ContentDelta::TextDelta { text } => assert_eq!(text, "Hello"),
                    _ => panic!("Wrong delta type"),
                }
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_parse_result_message() {
        let json = r#"{"type":"result","cost_usd":0.01,"input_tokens":100,"output_tokens":50,"stop_reason":"end_turn"}"#;
        let msg: CliOutputMessage = serde_json::from_str(json).unwrap();

        match msg {
            CliOutputMessage::Result(result) => {
                assert_eq!(result.cost_usd, Some(0.01));
                assert_eq!(result.input_tokens, Some(100));
                assert_eq!(result.output_tokens, Some(50));
                assert_eq!(result.stop_reason, Some("end_turn".to_string()));
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_config_builder() {
        let config = ClaudeCliConfig::new(PathBuf::from("/test"), "test-context")
            .with_model("claude-3-opus")
            .with_system_prompt("Be helpful")
            .bypass_permissions(false)
            .with_env("CUSTOM_VAR", "value");

        assert_eq!(config.cwd, PathBuf::from("/test"));
        assert_eq!(config.context, "test-context");
        assert_eq!(config.model, Some("claude-3-opus".to_string()));
        assert_eq!(config.system_prompt, Some("Be helpful".to_string()));
        assert!(!config.bypass_permissions);
        assert_eq!(config.env.get("CUSTOM_VAR"), Some(&"value".to_string()));
    }
}
