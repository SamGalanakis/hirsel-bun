//! ACP Bridge for Claude CLI.
//!
//! This module provides an ACP server that wraps the Claude CLI,
//! translating between the ACP JSON-RPC protocol and Claude CLI's
//! native JSON streaming protocol.
//!
//! ## Usage
//!
//! ```bash
//! hirsel __acp-bridge
//! ```
//!
//! This command acts as an ACP server, accepting JSON-RPC on stdin
//! and outputting ACP responses on stdout. It spawns the claude CLI
//! internally and handles protocol translation.

use crate::core::acp::{EnvVariable as InternalEnvVariable, MCPServerConfig};
use crate::core::claude_cli::{BridgeEvent, ClaudeCliBridge, ClaudeCliConfig};
use agent_client_protocol::{
    Agent, AgentSideConnection, AuthenticateRequest, AuthenticateResponse, CancelNotification,
    Client, ContentBlock, ContentChunk, ExtNotification, ExtRequest, ExtResponse, Implementation,
    InitializeRequest, InitializeResponse, LoadSessionRequest, LoadSessionResponse, McpServer,
    NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, ProtocolVersion,
    SessionId, SessionNotification, SessionUpdate, SetSessionModeRequest, SetSessionModeResponse,
    StopReason, TextContent, ToolCall, ToolCallId, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Shared state between the agent and the notification forwarder
struct SharedState {
    sessions: HashMap<String, SessionState>,
    notification_tx: Option<mpsc::UnboundedSender<SessionNotification>>,
}

/// State for an active session
struct SessionState {
    bridge: Option<ClaudeCliBridge>,
    event_rx: Option<mpsc::UnboundedReceiver<BridgeEvent>>,
    active_tools: HashMap<String, String>, // tool_call_id -> tool_name
}

/// ACP Agent implementation that wraps Claude CLI.
pub struct ClaudeAcpAgent {
    state: Rc<RefCell<SharedState>>,
}

impl ClaudeAcpAgent {
    pub fn new(notification_tx: mpsc::UnboundedSender<SessionNotification>) -> Self {
        Self {
            state: Rc::new(RefCell::new(SharedState {
                sessions: HashMap::new(),
                notification_tx: Some(notification_tx),
            })),
        }
    }

    /// Send a notification to the ACP client
    fn send_notification(&self, notification: SessionNotification) {
        let state = self.state.borrow();
        if let Some(ref tx) = state.notification_tx {
            if let Err(e) = tx.send(notification) {
                warn!("[acp-bridge] Failed to send notification: {:?}", e);
            }
        }
    }

    /// Convert ACP McpServer to internal MCPServerConfig
    fn convert_mcp_server(server: &McpServer) -> Option<MCPServerConfig> {
        match server {
            McpServer::Stdio(stdio) => {
                let env = if stdio.env.is_empty() {
                    None
                } else {
                    Some(
                        stdio
                            .env
                            .iter()
                            .map(|v| InternalEnvVariable {
                                name: v.name.clone(),
                                value: v.value.clone(),
                            })
                            .collect(),
                    )
                };
                Some(MCPServerConfig {
                    name: stdio.name.clone(),
                    command: stdio.command.to_string_lossy().to_string(),
                    args: stdio.args.clone(),
                    env,
                })
            }
            _ => {
                warn!("[acp-bridge] Unsupported MCP server type, skipping");
                None
            }
        }
    }

    /// Map tool name to ACP ToolKind
    fn tool_name_to_kind(tool_name: &str) -> ToolKind {
        match tool_name.to_lowercase().as_str() {
            "read" | "readtextfile" | "read_text_file" => ToolKind::Read,
            "edit" | "write" | "writetextfile" | "write_text_file" | "notebookedit" => {
                ToolKind::Edit
            }
            "bash" | "terminal" | "execute" | "createterminal" => ToolKind::Execute,
            "glob" | "grep" | "search" => ToolKind::Search,
            "webfetch" | "websearch" | "fetch" => ToolKind::Fetch,
            "task" => ToolKind::Think,
            _ => ToolKind::Other,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Agent for ClaudeAcpAgent {
    async fn initialize(
        &self,
        _args: InitializeRequest,
    ) -> agent_client_protocol::Result<InitializeResponse> {
        info!("[acp-bridge] Initialize request received");

        Ok(
            InitializeResponse::new(ProtocolVersion::LATEST).agent_info(Implementation::new(
                "hirsel-claude-bridge",
                env!("CARGO_PKG_VERSION"),
            )),
        )
    }

    async fn authenticate(
        &self,
        _args: AuthenticateRequest,
    ) -> agent_client_protocol::Result<AuthenticateResponse> {
        // No auth needed for local bridge
        Ok(AuthenticateResponse::default())
    }

    async fn new_session(
        &self,
        args: NewSessionRequest,
    ) -> agent_client_protocol::Result<NewSessionResponse> {
        let session_id = uuid::Uuid::new_v4().to_string();
        info!(
            "[acp-bridge] New session request: {} (cwd: {:?}, mcp_servers: {})",
            session_id,
            args.cwd,
            args.mcp_servers.len()
        );

        // Convert MCP servers from ACP format to internal format
        let mcp_servers: Vec<MCPServerConfig> = args
            .mcp_servers
            .iter()
            .filter_map(Self::convert_mcp_server)
            .collect();

        if !mcp_servers.is_empty() {
            info!(
                "[acp-bridge] Configuring {} MCP server(s): {:?}",
                mcp_servers.len(),
                mcp_servers.iter().map(|s| &s.name).collect::<Vec<_>>()
            );
        }

        // Create Claude CLI config
        let work_dir = args.cwd.clone();
        let config = ClaudeCliConfig::new(work_dir, format!("acp-session-{}", &session_id[..8]))
            .with_mcp_servers(mcp_servers)
            .bypass_permissions(false); // We handle permissions via ACP

        // Spawn Claude CLI bridge
        let (bridge, event_rx) = ClaudeCliBridge::spawn(config).map_err(|e| {
            error!("[acp-bridge] Failed to spawn Claude CLI: {}", e);
            agent_client_protocol::Error::internal_error()
        })?;

        // Store session state
        let session = SessionState {
            bridge: Some(bridge),
            event_rx: Some(event_rx),
            active_tools: HashMap::new(),
        };

        self.state
            .borrow_mut()
            .sessions
            .insert(session_id.clone(), session);

        Ok(NewSessionResponse::new(SessionId::new(session_id)))
    }

    async fn load_session(
        &self,
        _args: LoadSessionRequest,
    ) -> agent_client_protocol::Result<LoadSessionResponse> {
        // Session loading not supported
        Err(agent_client_protocol::Error::method_not_found())
    }

    async fn set_session_mode(
        &self,
        _args: SetSessionModeRequest,
    ) -> agent_client_protocol::Result<SetSessionModeResponse> {
        // Session mode changes not supported
        Ok(SetSessionModeResponse::default())
    }

    async fn prompt(&self, args: PromptRequest) -> agent_client_protocol::Result<PromptResponse> {
        let session_id = args.session_id.to_string();
        info!("[acp-bridge] Prompt request for session: {}", session_id);

        // Extract text content from prompt (prompt field, not message)
        let content = args
            .prompt
            .iter()
            .filter_map(|block| {
                if let ContentBlock::Text(text) = block {
                    Some(text.text.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        debug!("[acp-bridge] Prompt content: {} chars", content.len());

        // Send the prompt to Claude CLI - take bridge out to avoid holding borrow across await
        let mut bridge = {
            let mut state = self.state.borrow_mut();
            let session = state.sessions.get_mut(&session_id).ok_or_else(|| {
                error!("[acp-bridge] Session not found: {}", session_id);
                agent_client_protocol::Error::internal_error()
            })?;
            session.bridge.take().ok_or_else(|| {
                error!("[acp-bridge] Bridge already taken");
                agent_client_protocol::Error::internal_error()
            })?
        };

        let send_result = bridge.send_prompt(&content).await;

        // Put bridge back
        {
            let mut state = self.state.borrow_mut();
            if let Some(session) = state.sessions.get_mut(&session_id) {
                session.bridge = Some(bridge);
            }
        }

        send_result.map_err(|e| {
            error!("[acp-bridge] Failed to send prompt: {}", e);
            agent_client_protocol::Error::internal_error()
        })?;

        // Process events until message complete
        let mut stop_reason = StopReason::EndTurn;

        loop {
            // Take the receiver out of the session so we can await without holding the borrow
            let mut event_rx = {
                let mut state = self.state.borrow_mut();
                let session = state
                    .sessions
                    .get_mut(&session_id)
                    .ok_or_else(|| agent_client_protocol::Error::internal_error())?;
                session
                    .event_rx
                    .take()
                    .ok_or_else(|| agent_client_protocol::Error::internal_error())?
            };

            // Now we can await without holding the RefCell borrow
            let event = event_rx.recv().await;

            // Put the receiver back
            {
                let mut state = self.state.borrow_mut();
                if let Some(session) = state.sessions.get_mut(&session_id) {
                    session.event_rx = Some(event_rx);
                }
            }

            let Some(event) = event else {
                info!("[acp-bridge] Event stream ended");
                break;
            };

            match &event {
                BridgeEvent::TextDelta { text } => {
                    let notification = SessionNotification::new(
                        session_id.clone(),
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                            TextContent::new(text.clone()),
                        ))),
                    );
                    self.send_notification(notification);
                }
                BridgeEvent::ThinkingDelta { thinking } => {
                    let notification = SessionNotification::new(
                        session_id.clone(),
                        SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
                            TextContent::new(thinking.clone()),
                        ))),
                    );
                    self.send_notification(notification);
                }
                BridgeEvent::ToolCallStart {
                    tool_call_id,
                    tool_name,
                    input,
                } => {
                    // Track active tool
                    {
                        let mut state = self.state.borrow_mut();
                        if let Some(session) = state.sessions.get_mut(&session_id) {
                            session
                                .active_tools
                                .insert(tool_call_id.clone(), tool_name.clone());
                        }
                    }

                    let kind = Self::tool_name_to_kind(tool_name);
                    let raw_input = if input.is_null() {
                        None
                    } else {
                        Some(input.clone())
                    };

                    let tool_call =
                        ToolCall::new(ToolCallId::new(tool_call_id.clone()), tool_name.clone())
                            .kind(kind)
                            .status(ToolCallStatus::InProgress)
                            .raw_input(raw_input);

                    let notification = SessionNotification::new(
                        session_id.clone(),
                        SessionUpdate::ToolCall(tool_call),
                    );
                    self.send_notification(notification);
                }
                BridgeEvent::ToolCallComplete {
                    tool_call_id,
                    output,
                } => {
                    // Remove from active tools
                    {
                        let mut state = self.state.borrow_mut();
                        if let Some(session) = state.sessions.get_mut(&session_id) {
                            session.active_tools.remove(tool_call_id);
                        }
                    }

                    let mut fields = ToolCallUpdateFields::new().status(ToolCallStatus::Completed);
                    if let Some(out) = output {
                        fields = fields.raw_output(serde_json::Value::String(out.clone()));
                    }

                    let update = ToolCallUpdate::new(ToolCallId::new(tool_call_id.clone()), fields);

                    let notification = SessionNotification::new(
                        session_id.clone(),
                        SessionUpdate::ToolCallUpdate(update),
                    );
                    self.send_notification(notification);
                }
                BridgeEvent::ToolCallInputDelta {
                    tool_call_id,
                    partial_json: _,
                } => {
                    // Get tool name for title
                    let title = {
                        let state = self.state.borrow();
                        state
                            .sessions
                            .get(&session_id)
                            .and_then(|s| s.active_tools.get(tool_call_id))
                            .map(|name| format!("{} (streaming input...)", name))
                    };

                    if let Some(t) = title {
                        let fields = ToolCallUpdateFields::new().title(t);
                        let update =
                            ToolCallUpdate::new(ToolCallId::new(tool_call_id.clone()), fields);

                        let notification = SessionNotification::new(
                            session_id.clone(),
                            SessionUpdate::ToolCallUpdate(update),
                        );
                        self.send_notification(notification);
                    }
                }
                BridgeEvent::PermissionRequest {
                    request_id,
                    tool_name,
                    ..
                } => {
                    // Auto-approve all permissions in bridge mode
                    debug!(
                        "[acp-bridge] Auto-approving permission for tool: {} (request_id: {})",
                        tool_name, request_id
                    );
                    // Take bridge out to avoid holding borrow across await
                    let bridge_opt = {
                        let mut state = self.state.borrow_mut();
                        state
                            .sessions
                            .get_mut(&session_id)
                            .and_then(|s| s.bridge.take())
                    };
                    if let Some(mut bridge) = bridge_opt {
                        if let Err(e) = bridge.respond_permission_always(request_id).await {
                            warn!("[acp-bridge] Failed to respond to permission: {}", e);
                        }
                        // Put bridge back
                        let mut state = self.state.borrow_mut();
                        if let Some(session) = state.sessions.get_mut(&session_id) {
                            session.bridge = Some(bridge);
                        }
                    } else {
                        warn!("[acp-bridge] No bridge found for session: {}", session_id);
                    }
                }
                BridgeEvent::HookCallback { request_id, .. } => {
                    // Take bridge out to avoid holding borrow across await
                    let bridge_opt = {
                        let mut state = self.state.borrow_mut();
                        state
                            .sessions
                            .get_mut(&session_id)
                            .and_then(|s| s.bridge.take())
                    };
                    if let Some(mut bridge) = bridge_opt {
                        if let Err(e) = bridge.respond_hook(request_id, serde_json::json!({})).await
                        {
                            warn!("[acp-bridge] Failed to respond to hook: {}", e);
                        }
                        // Put bridge back
                        let mut state = self.state.borrow_mut();
                        if let Some(session) = state.sessions.get_mut(&session_id) {
                            session.bridge = Some(bridge);
                        }
                    }
                }
                BridgeEvent::SessionInit { session_id: sid } => {
                    info!("[acp-bridge] Claude session initialized: {}", sid);
                }
                BridgeEvent::MessageComplete {
                    stop_reason: sr, ..
                } => {
                    info!("[acp-bridge] Message complete: {:?}", sr);
                    // Map stop reasons - ACP has limited variants
                    stop_reason = match sr.as_deref() {
                        Some("max_tokens") => StopReason::MaxTokens,
                        Some("max_turn_requests") => StopReason::MaxTurnRequests,
                        _ => StopReason::EndTurn, // Default to EndTurn for end_turn, tool_use, stop_sequence, etc.
                    };
                    break;
                }
                BridgeEvent::Error { message } => {
                    error!("[acp-bridge] Error from CLI: {}", message);
                    // Don't break - continue processing
                }
                BridgeEvent::ProcessExited { code } => {
                    info!("[acp-bridge] Process exited with code: {:?}", code);
                    break;
                }
            }
        }

        Ok(PromptResponse::new(stop_reason))
    }

    async fn cancel(&self, args: CancelNotification) -> agent_client_protocol::Result<()> {
        let session_id = args.session_id.to_string();
        info!("[acp-bridge] Cancel request for session: {}", session_id);

        // Clean up session
        let mut state = self.state.borrow_mut();
        if let Some(session) = state.sessions.remove(&session_id) {
            if let Some(bridge) = session.bridge {
                bridge.cleanup();
            }
        }

        Ok(())
    }

    async fn ext_method(&self, _args: ExtRequest) -> agent_client_protocol::Result<ExtResponse> {
        Err(agent_client_protocol::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: ExtNotification) -> agent_client_protocol::Result<()> {
        Ok(())
    }
}

/// Run the ACP bridge server on stdin/stdout.
pub fn run_acp_bridge() -> anyhow::Result<()> {
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        tokio::task::LocalSet::new()
            .run_until(async {
                info!("[acp-bridge] Starting ACP bridge server");

                // Create notification channel
                let (notification_tx, mut notification_rx) = mpsc::unbounded_channel();

                // Create agent with notification sender
                let agent = ClaudeAcpAgent::new(notification_tx);

                // Create stdin/stdout streams
                let stdin = tokio::io::stdin();
                let stdout = tokio::io::stdout();

                let stdin_compat = stdin.compat();
                let stdout_compat = stdout.compat_write();

                // Create ACP connection
                // The agent handles incoming requests (initialize, new_session, prompt)
                // The connection implements Client for sending notifications
                let (conn, io_task) =
                    AgentSideConnection::new(agent, stdout_compat, stdin_compat, |fut| {
                        tokio::task::spawn_local(fut);
                    });

                // Spawn notification forwarder
                // This reads from the channel and sends via the connection
                tokio::task::spawn_local(async move {
                    while let Some(notification) = notification_rx.recv().await {
                        if let Err(e) = conn.session_notification(notification).await {
                            warn!(
                                "[acp-bridge] Failed to send notification via connection: {:?}",
                                e
                            );
                            break;
                        }
                    }
                    debug!("[acp-bridge] Notification forwarder exiting");
                });

                // Wait for IO task to complete (client disconnected)
                if let Err(e) = io_task.await {
                    error!("[acp-bridge] IO error: {:?}", e);
                }

                info!("[acp-bridge] ACP bridge server exiting");
            })
            .await
    });

    Ok(())
}
