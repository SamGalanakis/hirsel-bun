//! Direct chat session management for hirsel.
//!
//! This module provides functionality for spawning interactive chat sessions
//! with AI agents via ACP. Unlike worker sessions, these are designed for
//! direct human-to-agent conversation with context awareness.
//!
//! Key features:
//! - Permission routing: auto-approve hirsel-related, prompt user for others
//! - Context injection: prepend UI state to user messages
//! - Full hirsel MCP access for the agent

use agent_client_protocol::{
    Agent, Client, ClientSideConnection, ContentBlock, CreateTerminalRequest,
    CreateTerminalResponse, EnvVariable, Implementation, InitializeRequest,
    KillTerminalCommandRequest, KillTerminalCommandResponse, McpServer, McpServerStdio,
    NewSessionRequest, PermissionOptionKind, PromptRequest, ProtocolVersion, ReadTextFileRequest,
    ReadTextFileResponse, ReleaseTerminalRequest, ReleaseTerminalResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, TerminalExitStatus, TerminalId,
    TerminalOutputRequest, TerminalOutputResponse, TextContent, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse, WriteTextFileRequest, WriteTextFileResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use tracing::{debug, error, info};
use uuid::Uuid;

use super::acp::{AcpChild, AcpSpawnConfig};
use super::credentials::ForwardedCredentials;

/// Result type for chat session operations
type Result<T> = std::result::Result<T, ChatSessionError>;

/// Errors that can occur in chat sessions
#[derive(Debug, thiserror::Error)]
pub enum ChatSessionError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Session already exists: {0}")]
    SessionExists(String),

    #[error("Failed to spawn agent: {0}")]
    SpawnFailed(String),

    #[error("ACP error: {0}")]
    Acp(#[from] agent_client_protocol::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Channel error: {0}")]
    Channel(String),

    #[error("Permission denied by user")]
    PermissionDenied,

    #[error("Permission request timed out")]
    PermissionTimeout,
}

/// UI context to inject before user messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UIContext {
    /// Currently selected run name
    pub selected_run: Option<String>,
    /// Currently selected worker name
    pub selected_worker: Option<String>,
    /// Current UI section (e.g., "tasks", "workers", "eval", "spec", "chat")
    pub ui_section: String,
    /// Additional context (e.g., selected task ID)
    pub extra: Option<HashMap<String, String>>,
}

impl UIContext {
    /// Format context as a system message prefix
    pub fn to_context_prefix(&self) -> String {
        let mut parts = vec!["<ui-context>".to_string()];

        if let Some(ref run) = self.selected_run {
            parts.push(format!("Selected run: {}", run));
        } else {
            parts.push("No run selected".to_string());
        }

        if let Some(ref worker) = self.selected_worker {
            parts.push(format!("Selected worker: {}", worker));
        }

        parts.push(format!("UI section: {}", self.ui_section));

        if let Some(ref extra) = self.extra {
            for (k, v) in extra {
                parts.push(format!("{}: {}", k, v));
            }
        }

        parts.push("</ui-context>".to_string());
        parts.push(String::new()); // Empty line before user message

        parts.join("\n")
    }
}

/// A permission request pending user response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermission {
    pub request_id: String,
    pub session_id: String,
    pub title: String,
    pub description: Option<String>,
    pub options: Vec<PermissionOption>,
}

/// A permission option
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub option_id: String,
    pub label: String,
    pub kind: String,
}

/// User's response to a permission request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponse {
    pub request_id: String,
    pub option_id: String,
}

/// Chat event emitted to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ChatEvent {
    /// Text chunk from assistant
    #[serde(rename_all = "camelCase")]
    TextDelta { session_id: String, text: String },
    /// Thinking/reasoning chunk
    #[serde(rename_all = "camelCase")]
    ThinkingDelta { session_id: String, text: String },
    /// Tool call started
    #[serde(rename_all = "camelCase")]
    ToolCallStart {
        session_id: String,
        tool_call_id: String,
        title: String,
        kind: Option<String>,
        input: Option<String>,
    },
    /// Tool call updated (status, output)
    #[serde(rename_all = "camelCase")]
    ToolCallUpdate {
        session_id: String,
        tool_call_id: String,
        status: String,
        title: Option<String>,
        output: Option<String>,
    },
    /// Permission request (needs user response)
    #[serde(rename_all = "camelCase")]
    PermissionRequest {
        session_id: String,
        request: PendingPermission,
    },
    /// Message completed
    #[serde(rename_all = "camelCase")]
    MessageComplete { session_id: String },
    /// Error occurred
    #[serde(rename_all = "camelCase")]
    Error { session_id: String, message: String },
    /// Session ended
    #[serde(rename_all = "camelCase")]
    SessionEnded { session_id: String },
}

/// Terminal handle for tracking spawned terminals
struct TerminalHandle {
    child: Child,
    output: String,
}

/// Internal state for a chat session
struct ChatSessionState {
    _session_id: String,
    _agent_session_id: Option<String>,
    _working_dir: PathBuf,
    acp_child: Option<AcpChild>,
    terminals: HashMap<TerminalId, TerminalHandle>,
    terminal_counter: AtomicU64,
}

/// ACP client implementation for chat sessions
pub struct ChatClient {
    session_id: String,
    event_tx: mpsc::UnboundedSender<ChatEvent>,
    state: Arc<Mutex<ChatSessionState>>,
    /// Pending permission requests waiting for user response
    pending_permissions: Arc<Mutex<HashMap<String, oneshot::Sender<PermissionResponse>>>>,
}

impl ChatClient {
    fn new(
        session_id: String,
        event_tx: mpsc::UnboundedSender<ChatEvent>,
        state: Arc<Mutex<ChatSessionState>>,
    ) -> Self {
        Self {
            session_id,
            event_tx,
            state,
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if a permission request is hirsel-related and should be auto-approved
    fn is_hirsel_related(&self, args: &RequestPermissionRequest) -> bool {
        // Get title from tool_call if available
        let title = args.tool_call.fields.title.as_deref().unwrap_or("");
        let title_lower = title.to_lowercase();

        // Auto-approve hirsel MCP tool calls
        title_lower.contains("hirsel") ||
        // Common hirsel tool patterns
        title_lower.contains("task_") ||
        title_lower.contains("msg_") ||
        title_lower.contains("work_done")
    }

    /// Send a permission request to the frontend and wait for response
    async fn prompt_user_permission(
        &self,
        args: &RequestPermissionRequest,
    ) -> std::result::Result<PermissionResponse, ChatSessionError> {
        let request_id = Uuid::new_v4().to_string();

        // Get title from tool_call
        let title = args
            .tool_call
            .fields
            .title
            .clone()
            .unwrap_or_else(|| "Permission Request".to_string());

        // Create the pending permission
        let pending = PendingPermission {
            request_id: request_id.clone(),
            session_id: self.session_id.clone(),
            title,
            description: None,
            options: args
                .options
                .iter()
                .map(|o| PermissionOption {
                    option_id: o.option_id.to_string(),
                    label: o.name.clone(),
                    kind: format!("{:?}", o.kind),
                })
                .collect(),
        };

        // Create response channel
        let (tx, rx) = oneshot::channel();

        // Store the sender
        {
            let mut permissions = self.pending_permissions.lock().await;
            permissions.insert(request_id.clone(), tx);
        }

        // Emit permission request event
        let _ = self.event_tx.send(ChatEvent::PermissionRequest {
            session_id: self.session_id.clone(),
            request: pending,
        });

        // Wait for response with timeout
        match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(ChatSessionError::Channel(
                "Permission channel closed".into(),
            )),
            Err(_) => {
                // Clean up on timeout
                let mut permissions = self.pending_permissions.lock().await;
                permissions.remove(&request_id);
                Err(ChatSessionError::PermissionTimeout)
            }
        }
    }

    /// Respond to a pending permission request
    pub async fn respond_to_permission(&self, response: PermissionResponse) -> Result<()> {
        let mut permissions = self.pending_permissions.lock().await;
        if let Some(tx) = permissions.remove(&response.request_id) {
            tx.send(response).map_err(|_| {
                ChatSessionError::Channel("Failed to send permission response".into())
            })?;
        }
        Ok(())
    }
}

#[async_trait::async_trait(?Send)]
impl Client for ChatClient {
    async fn request_permission(
        &self,
        args: RequestPermissionRequest,
    ) -> std::result::Result<RequestPermissionResponse, agent_client_protocol::Error> {
        // Auto-approve hirsel-related permissions
        if self.is_hirsel_related(&args) {
            let title = args.tool_call.fields.title.as_deref().unwrap_or("unknown");
            debug!(
                "[chat:{}] Auto-approving hirsel permission: {}",
                self.session_id, title
            );

            let option_id = args
                .options
                .iter()
                .find(|o| o.kind == PermissionOptionKind::AllowAlways)
                .or_else(|| {
                    args.options
                        .iter()
                        .find(|o| o.kind == PermissionOptionKind::AllowOnce)
                })
                .map(|o| o.option_id.clone())
                .unwrap_or_else(|| "allow".into());

            return Ok(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
            ));
        }

        // For other permissions, prompt the user
        let title = args.tool_call.fields.title.as_deref().unwrap_or("unknown");
        debug!(
            "[chat:{}] Requesting user permission: {}",
            self.session_id, title
        );

        match self.prompt_user_permission(&args).await {
            Ok(response) => Ok(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                    response.option_id,
                )),
            )),
            Err(ChatSessionError::PermissionDenied) => {
                // Find a non-allow option, or use first option as fallback
                let option_id = args
                    .options
                    .iter()
                    .find(|o| {
                        o.kind != PermissionOptionKind::AllowAlways
                            && o.kind != PermissionOptionKind::AllowOnce
                    })
                    .or_else(|| args.options.first())
                    .map(|o| o.option_id.clone())
                    .unwrap_or_else(|| "deny".into());

                Ok(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
                ))
            }
            Err(e) => {
                error!("[chat:{}] Permission error: {}", self.session_id, e);
                Err(agent_client_protocol::Error::internal_error())
            }
        }
    }

    async fn session_notification(
        &self,
        args: SessionNotification,
    ) -> std::result::Result<(), agent_client_protocol::Error> {
        eprintln!(
            "[CHAT] session_notification called for {}: {:?}",
            self.session_id, args.update
        );
        match &args.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let ContentBlock::Text(text) = &chunk.content {
                    eprintln!("[CHAT] TextDelta: '{}'", text.text);
                    let _ = self.event_tx.send(ChatEvent::TextDelta {
                        session_id: self.session_id.clone(),
                        text: text.text.clone(),
                    });
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                if let ContentBlock::Text(text) = &chunk.content {
                    let _ = self.event_tx.send(ChatEvent::ThinkingDelta {
                        session_id: self.session_id.clone(),
                        text: text.text.clone(),
                    });
                }
            }
            SessionUpdate::ToolCall(tc) => {
                let kind = match tc.kind {
                    agent_client_protocol::ToolKind::Read => Some("read"),
                    agent_client_protocol::ToolKind::Edit => Some("edit"),
                    agent_client_protocol::ToolKind::Delete => Some("delete"),
                    agent_client_protocol::ToolKind::Move => Some("move"),
                    agent_client_protocol::ToolKind::Search => Some("search"),
                    agent_client_protocol::ToolKind::Execute => Some("execute"),
                    agent_client_protocol::ToolKind::Think => Some("think"),
                    agent_client_protocol::ToolKind::Fetch => Some("fetch"),
                    agent_client_protocol::ToolKind::SwitchMode => Some("switch_mode"),
                    _ => None,
                };

                // Serialize input if present
                let input = tc
                    .raw_input
                    .as_ref()
                    .and_then(|v| serde_json::to_string(v).ok());

                let _ = self.event_tx.send(ChatEvent::ToolCallStart {
                    session_id: self.session_id.clone(),
                    tool_call_id: tc.tool_call_id.to_string(),
                    title: tc.title.clone(),
                    kind: kind.map(|s| s.to_string()),
                    input,
                });
            }
            SessionUpdate::ToolCallUpdate(update) => {
                let status = update
                    .fields
                    .status
                    .map(|s| match s {
                        agent_client_protocol::ToolCallStatus::Pending => "pending",
                        agent_client_protocol::ToolCallStatus::InProgress => "in_progress",
                        agent_client_protocol::ToolCallStatus::Completed => "completed",
                        agent_client_protocol::ToolCallStatus::Failed => "failed",
                        _ => "unknown",
                    })
                    .unwrap_or("unknown");

                // Extract output using shared utility
                let output = crate::core::acp::extract_tool_output(&update.fields);

                let _ = self.event_tx.send(ChatEvent::ToolCallUpdate {
                    session_id: self.session_id.clone(),
                    tool_call_id: update.tool_call_id.to_string(),
                    status: status.to_string(),
                    title: update.fields.title.clone(),
                    output,
                });
            }
            _ => {}
        }
        Ok(())
    }

    async fn read_text_file(
        &self,
        args: ReadTextFileRequest,
    ) -> std::result::Result<ReadTextFileResponse, agent_client_protocol::Error> {
        let path = Path::new(&args.path);
        match std::fs::read_to_string(path) {
            Ok(mut content) => {
                if args.line.is_some() || args.limit.is_some() {
                    let lines: Vec<&str> = content.lines().collect();
                    let start = args.line.map(|l| l as usize).unwrap_or(0);
                    let end = args
                        .limit
                        .map(|l| start + l as usize)
                        .unwrap_or(lines.len());
                    content = lines[start.min(lines.len())..end.min(lines.len())].join("\n");
                }
                Ok(ReadTextFileResponse::new(content))
            }
            Err(_) => Err(agent_client_protocol::Error::internal_error()),
        }
    }

    async fn write_text_file(
        &self,
        args: WriteTextFileRequest,
    ) -> std::result::Result<WriteTextFileResponse, agent_client_protocol::Error> {
        let path = Path::new(&args.path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(path, &args.content) {
            Ok(()) => Ok(WriteTextFileResponse::new()),
            Err(_) => Err(agent_client_protocol::Error::internal_error()),
        }
    }

    async fn create_terminal(
        &self,
        args: CreateTerminalRequest,
    ) -> std::result::Result<CreateTerminalResponse, agent_client_protocol::Error> {
        let mut state = self.state.lock().await;
        let id = state.terminal_counter.fetch_add(1, Ordering::SeqCst);
        let terminal_id = TerminalId::new(format!("term_{}", id));

        let mut cmd = Command::new(&args.command);
        if !args.args.is_empty() {
            cmd.args(&args.args);
        }
        if let Some(ref cwd) = args.cwd {
            cmd.current_dir(cwd);
        }
        for e in &args.env {
            cmd.env(&e.name, &e.value);
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        match cmd.spawn() {
            Ok(child) => {
                state.terminals.insert(
                    terminal_id.clone(),
                    TerminalHandle {
                        child,
                        output: String::new(),
                    },
                );
                Ok(CreateTerminalResponse::new(terminal_id))
            }
            Err(_) => Err(agent_client_protocol::Error::internal_error()),
        }
    }

    async fn terminal_output(
        &self,
        args: TerminalOutputRequest,
    ) -> std::result::Result<TerminalOutputResponse, agent_client_protocol::Error> {
        let mut state = self.state.lock().await;
        if let Some(handle) = state.terminals.get_mut(&args.terminal_id) {
            let exit_status = handle
                .child
                .try_wait()
                .ok()
                .flatten()
                .map(|s| TerminalExitStatus::new().exit_code(s.code().map(|c| c as u32)));
            let mut response = TerminalOutputResponse::new(handle.output.clone(), false);
            if let Some(status) = exit_status {
                response = response.exit_status(status);
            }
            Ok(response)
        } else {
            Err(agent_client_protocol::Error::internal_error())
        }
    }

    async fn release_terminal(
        &self,
        args: ReleaseTerminalRequest,
    ) -> std::result::Result<ReleaseTerminalResponse, agent_client_protocol::Error> {
        let mut state = self.state.lock().await;
        if let Some(mut handle) = state.terminals.remove(&args.terminal_id) {
            let _ = handle.child.kill().await;
        }
        Ok(ReleaseTerminalResponse::new())
    }

    async fn wait_for_terminal_exit(
        &self,
        args: WaitForTerminalExitRequest,
    ) -> std::result::Result<WaitForTerminalExitResponse, agent_client_protocol::Error> {
        let mut state = self.state.lock().await;
        if let Some(handle) = state.terminals.get_mut(&args.terminal_id) {
            match handle.child.wait().await {
                Ok(status) => {
                    let exit_status =
                        TerminalExitStatus::new().exit_code(status.code().map(|c| c as u32));
                    Ok(WaitForTerminalExitResponse::new(exit_status))
                }
                Err(_) => Err(agent_client_protocol::Error::internal_error()),
            }
        } else {
            Err(agent_client_protocol::Error::internal_error())
        }
    }

    async fn kill_terminal_command(
        &self,
        args: KillTerminalCommandRequest,
    ) -> std::result::Result<KillTerminalCommandResponse, agent_client_protocol::Error> {
        let mut state = self.state.lock().await;
        if let Some(handle) = state.terminals.get_mut(&args.terminal_id) {
            let _ = handle.child.kill().await;
        }
        Ok(KillTerminalCommandResponse::new())
    }
}

/// Configuration for starting a chat session
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSessionConfig {
    /// Agent command (e.g., ["claude", "acp"])
    pub agent_command: Vec<String>,
    /// Working directory for the agent
    pub working_dir: Option<String>,
    /// Current run name (for hirsel MCP access)
    pub run_name: Option<String>,
    /// System prompt to prepend
    pub system_prompt: Option<String>,
    /// Credentials to forward to the agent process
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<ForwardedCredentials>,
}

/// Message sent to a chat session task
enum SessionCommand {
    SendMessage {
        content: String,
        context: Option<UIContext>,
    },
    RespondPermission(PermissionResponse),
    Stop,
}

/// Global chat session manager
pub struct ChatSessionManager {
    /// Channel senders for each session's command loop
    command_txs: RwLock<HashMap<String, mpsc::UnboundedSender<SessionCommand>>>,
    /// Event receivers that Tauri can listen to
    event_txs: RwLock<HashMap<String, mpsc::UnboundedSender<ChatEvent>>>,
}

impl ChatSessionManager {
    pub fn new() -> Self {
        Self {
            command_txs: RwLock::new(HashMap::new()),
            event_txs: RwLock::new(HashMap::new()),
        }
    }

    /// Start a new chat session
    ///
    /// This spawns the session in a dedicated thread with a LocalSet
    /// to handle the non-Send ACP connection.
    pub async fn start_session(
        &self,
        config: ChatSessionConfig,
    ) -> Result<(String, mpsc::UnboundedReceiver<ChatEvent>)> {
        let session_id = Uuid::new_v4().to_string();
        info!("[chat:{}] Starting chat session", session_id);

        // Create channels
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        // Store channels
        {
            let mut txs = self.event_txs.write().await;
            txs.insert(session_id.clone(), event_tx.clone());
        }
        {
            let mut txs = self.command_txs.write().await;
            txs.insert(session_id.clone(), cmd_tx);
        }

        // Spawn the session in a dedicated thread with LocalSet
        let session_id_clone = session_id.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create runtime");

            rt.block_on(async {
                let local = tokio::task::LocalSet::new();
                local
                    .run_until(run_chat_session_loop(
                        session_id_clone,
                        config,
                        event_tx,
                        cmd_rx,
                    ))
                    .await;
            });
        });

        Ok((session_id, event_rx))
    }

    /// Send a message to a chat session
    pub async fn send_message(
        &self,
        session_id: &str,
        content: String,
        context: Option<UIContext>,
    ) -> Result<()> {
        let txs = self.command_txs.read().await;
        let tx = txs
            .get(session_id)
            .ok_or_else(|| ChatSessionError::SessionNotFound(session_id.to_string()))?;

        tx.send(SessionCommand::SendMessage { content, context })
            .map_err(|_| ChatSessionError::Channel("Session closed".into()))?;

        Ok(())
    }

    /// Respond to a permission request
    pub async fn respond_to_permission(
        &self,
        session_id: &str,
        response: PermissionResponse,
    ) -> Result<()> {
        let txs = self.command_txs.read().await;
        let tx = txs
            .get(session_id)
            .ok_or_else(|| ChatSessionError::SessionNotFound(session_id.to_string()))?;

        tx.send(SessionCommand::RespondPermission(response))
            .map_err(|_| ChatSessionError::Channel("Session closed".into()))?;

        Ok(())
    }

    /// Stop a chat session
    pub async fn stop_session(&self, session_id: &str) -> Result<()> {
        info!("[chat:{}] Stopping session", session_id);

        // Send stop command
        {
            let txs = self.command_txs.read().await;
            if let Some(tx) = txs.get(session_id) {
                let _ = tx.send(SessionCommand::Stop);
            }
        }

        // Clean up channels
        {
            let mut txs = self.command_txs.write().await;
            txs.remove(session_id);
        }
        {
            let mut txs = self.event_txs.write().await;
            txs.remove(session_id);
        }

        Ok(())
    }

    /// Get event sender for a session (for external event injection)
    pub async fn get_event_tx(&self, session_id: &str) -> Option<mpsc::UnboundedSender<ChatEvent>> {
        let txs = self.event_txs.read().await;
        txs.get(session_id).cloned()
    }

    /// List active sessions
    pub async fn list_sessions(&self) -> Vec<String> {
        let txs = self.command_txs.read().await;
        txs.keys().cloned().collect()
    }
}

/// Run the chat session loop in a LocalSet
async fn run_chat_session_loop(
    session_id: String,
    config: ChatSessionConfig,
    event_tx: mpsc::UnboundedSender<ChatEvent>,
    mut cmd_rx: mpsc::UnboundedReceiver<SessionCommand>,
) {
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    // Determine working directory
    let working_dir = config
        .working_dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Create session state
    let state = Arc::new(Mutex::new(ChatSessionState {
        _session_id: session_id.clone(),
        _agent_session_id: None,
        _working_dir: working_dir.clone(),
        acp_child: None,
        terminals: HashMap::new(),
        terminal_counter: AtomicU64::new(0),
    }));

    // Create client
    let client = Arc::new(ChatClient::new(
        session_id.clone(),
        event_tx.clone(),
        state.clone(),
    ));

    // Spawn agent process using AcpChild for automatic cleanup
    let mut spawn_config = AcpSpawnConfig::new(
        config.agent_command.clone(),
        working_dir.clone(),
        format!("chat:{}", session_id),
    )
    .bypass_permissions(false); // Chat sessions need interactive permission handling

    // Apply forwarded credentials as environment variables
    if let Some(ref creds) = config.credentials {
        if let Some(ref token) = creds.claude_access_token {
            spawn_config = spawn_config.with_env("CLAUDE_ACCESS_TOKEN", token);
        }
        if let Some(ref key) = creds.anthropic_api_key {
            spawn_config = spawn_config.with_env("ANTHROPIC_API_KEY", key);
        }
    }

    let mut acp_child = match AcpChild::spawn(spawn_config) {
        Ok(c) => c,
        Err(e) => {
            let _ = event_tx.send(ChatEvent::Error {
                session_id: session_id.clone(),
                message: format!("Failed to spawn agent: {}", e),
            });
            return;
        }
    };

    let stdin = match acp_child.take_stdin() {
        Some(s) => s,
        None => {
            let _ = event_tx.send(ChatEvent::Error {
                session_id: session_id.clone(),
                message: "Failed to get stdin".into(),
            });
            return;
        }
    };
    let stdout = match acp_child.take_stdout() {
        Some(s) => s,
        None => {
            let _ = event_tx.send(ChatEvent::Error {
                session_id: session_id.clone(),
                message: "Failed to get stdout".into(),
            });
            return;
        }
    };

    // Store AcpChild in state
    {
        let mut s = state.lock().await;
        s.acp_child = Some(acp_child);
    }

    // Convert tokio streams to futures-compatible
    let stdin_compat = stdin.compat_write();
    let stdout_compat = stdout.compat();

    // Create ACP connection
    let (conn, io_task) =
        ClientSideConnection::new(client.clone(), stdin_compat, stdout_compat, |fut| {
            tokio::task::spawn_local(fut);
        });

    // Spawn IO task
    let session_id_clone = session_id.clone();
    let event_tx_clone = event_tx.clone();
    tokio::task::spawn_local(async move {
        if let Err(e) = io_task.await {
            error!("[chat:{}] ACP IO error: {:?}", session_id_clone, e);
            let _ = event_tx_clone.send(ChatEvent::Error {
                session_id: session_id_clone.clone(),
                message: format!("ACP IO error: {:?}", e),
            });
        }
    });

    // Initialize ACP
    let init_request = InitializeRequest::new(ProtocolVersion::LATEST).client_info(
        Implementation::new("hirsel-chat", env!("CARGO_PKG_VERSION")),
    );
    let init_result = match conn.initialize(init_request).await {
        Ok(r) => r,
        Err(e) => {
            let _ = event_tx.send(ChatEvent::Error {
                session_id: session_id.clone(),
                message: format!("ACP init failed: {:?}", e),
            });
            return;
        }
    };
    info!("[chat:{}] ACP initialized: {:?}", session_id, init_result);

    // Build MCP servers list
    let mut mcp_servers = Vec::new();

    // Add hirsel MCP if run is specified
    if let Some(ref run_name) = config.run_name {
        let mcp_config = super::acp::create_hirsel_mcp_config(run_name, "assistant", None);
        let mut mcp_stdio =
            McpServerStdio::new(&mcp_config.name, &mcp_config.command).args(mcp_config.args);
        if let Some(vars) = mcp_config.env {
            let env_vars: Vec<EnvVariable> = vars
                .into_iter()
                .map(|v| EnvVariable::new(&v.name, &v.value))
                .collect();
            mcp_stdio = mcp_stdio.env(env_vars);
        }
        mcp_servers.push(McpServer::Stdio(mcp_stdio));
    }

    // Create new session
    let session_request =
        NewSessionRequest::new(working_dir.to_string_lossy().to_string()).mcp_servers(mcp_servers);
    let session = match conn.new_session(session_request).await {
        Ok(s) => s,
        Err(e) => {
            let _ = event_tx.send(ChatEvent::Error {
                session_id: session_id.clone(),
                message: format!("Failed to create session: {:?}", e),
            });
            return;
        }
    };
    let agent_session_id = session.session_id.clone();
    info!(
        "[chat:{}] Created agent session: {}",
        session_id, agent_session_id
    );

    // Store system prompt for later use (prepend to first user message)
    // We don't send it as a prompt() because that triggers a model response
    let system_prompt = config.system_prompt.clone();
    let mut first_message = true;

    // Process commands
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            SessionCommand::SendMessage { content, context } => {
                // Build the full message with context prefix
                let mut full_content = if let Some(ctx) = context {
                    format!("{}{}", ctx.to_context_prefix(), content)
                } else {
                    content
                };

                // Prepend system prompt to first message only
                if first_message {
                    if let Some(ref sp) = system_prompt {
                        full_content = format!("<system>\n{}\n</system>\n\n{}", sp, full_content);
                    }
                    first_message = false;
                }

                info!(
                    "[chat:{}] Sending message: {} chars",
                    session_id,
                    full_content.len()
                );

                let prompt_request = PromptRequest::new(
                    agent_session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(full_content))],
                );
                match conn.prompt(prompt_request).await {
                    Ok(response) => {
                        info!(
                            "[chat:{}] Message response: {:?}",
                            session_id, response.stop_reason
                        );
                    }
                    Err(e) => {
                        error!("[chat:{}] Failed to send message: {:?}", session_id, e);
                        let _ = event_tx.send(ChatEvent::Error {
                            session_id: session_id.clone(),
                            message: format!("Failed to send message: {:?}", e),
                        });
                    }
                }

                // Signal message complete
                let _ = event_tx.send(ChatEvent::MessageComplete {
                    session_id: session_id.clone(),
                });
            }
            SessionCommand::RespondPermission(response) => {
                // Forward to client
                if let Err(e) = client.respond_to_permission(response).await {
                    error!(
                        "[chat:{}] Failed to respond to permission: {}",
                        session_id, e
                    );
                }
            }
            SessionCommand::Stop => {
                info!("[chat:{}] Received stop command", session_id);
                break;
            }
        }
    }

    // Clean up - AcpChild handles process group cleanup (SIGTERM -> wait -> SIGKILL)
    {
        let mut s = state.lock().await;
        if let Some(ref mut acp_child) = s.acp_child {
            let _ = acp_child.kill().await;
        }
        // AcpChild::Drop will also run cleanup when it goes out of scope
    }

    let _ = event_tx.send(ChatEvent::SessionEnded {
        session_id: session_id.clone(),
    });

    info!("[chat:{}] Session ended", session_id);
}

impl Default for ChatSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_context_to_prefix() {
        let ctx = UIContext {
            selected_run: Some("my-run".to_string()),
            selected_worker: Some("alpha".to_string()),
            ui_section: "tasks".to_string(),
            extra: None,
        };

        let prefix = ctx.to_context_prefix();
        assert!(prefix.contains("<ui-context>"));
        assert!(prefix.contains("Selected run: my-run"));
        assert!(prefix.contains("Selected worker: alpha"));
        assert!(prefix.contains("UI section: tasks"));
        assert!(prefix.contains("</ui-context>"));
    }

    #[test]
    fn test_ui_context_no_run() {
        let ctx = UIContext {
            selected_run: None,
            selected_worker: None,
            ui_section: "home".to_string(),
            extra: None,
        };

        let prefix = ctx.to_context_prefix();
        assert!(prefix.contains("No run selected"));
    }
}
