//! ACP client implementation for hirsel workers.
//!
//! This module implements the Client trait from agent-client-protocol
//! to handle agent requests for permissions, file operations, and terminals.

use agent_client_protocol::{
    Agent, Client, ClientSideConnection,
    CreateTerminalRequest, CreateTerminalResponse,
    KillTerminalCommandRequest, KillTerminalCommandResponse,
    ReadTextFileRequest, ReadTextFileResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse,
    RequestPermissionRequest, RequestPermissionResponse, RequestPermissionOutcome,
    SessionNotification, SessionUpdate,
    TerminalOutputRequest, TerminalOutputResponse, TerminalExitStatus,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse,
    WriteTextFileRequest, WriteTextFileResponse,
    InitializeRequest, NewSessionRequest, SetSessionModeRequest, PromptRequest,
    McpServer, McpServerStdio, ContentBlock, TextContent, Implementation, EnvVariable,
    ProtocolVersion, PermissionOptionKind, SelectedPermissionOutcome, TerminalId,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, info, error};

use crate::core::acp::{collect_agent_env, create_hirsel_mcp_config};

/// Result type for ACP operations
type Result<T> = std::result::Result<T, agent_client_protocol::Error>;

/// Terminal handle for tracking spawned terminals
struct TerminalHandle {
    child: Child,
    output: String,
}

/// Hirsel's implementation of the ACP Client trait.
/// Handles agent requests for permissions, file operations, and terminals.
pub struct HirselClient {
    worker_name: String,
    log_file: PathBuf,
    db_path: PathBuf,
    terminals: Mutex<HashMap<TerminalId, TerminalHandle>>,
    terminal_counter: AtomicU64,
}

impl HirselClient {
    pub fn new(worker_name: &str, log_file: &Path, db_path: &Path) -> Self {
        Self {
            worker_name: worker_name.to_string(),
            log_file: log_file.to_path_buf(),
            db_path: db_path.to_path_buf(),
            terminals: Mutex::new(HashMap::new()),
            terminal_counter: AtomicU64::new(0),
        }
    }

    fn log(&self, message: &str) {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file)
        {
            use std::io::Write;
            let _ = writeln!(file, "{}", message);
        }
    }

    fn get_state(&self) -> Option<crate::core::state::SQLiteState> {
        crate::core::state::SQLiteState::new(self.db_path.clone()).ok()
    }
}

#[async_trait::async_trait(?Send)]
impl Client for HirselClient {
    async fn request_permission(
        &self,
        args: RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse> {
        // Auto-approve all permission requests (bypassPermissions mode)
        let option_id = args.options.iter()
            .find(|o| o.kind == PermissionOptionKind::AllowAlways)
            .or_else(|| args.options.iter().find(|o| o.kind == PermissionOptionKind::AllowOnce))
            .map(|o| o.option_id.clone())
            .unwrap_or_else(|| "allow".into());

        debug!("[{}] Auto-approving permission", self.worker_name);

        Ok(RequestPermissionResponse::new(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id))
        ))
    }

    async fn session_notification(&self, args: SessionNotification) -> Result<()> {
        use crate::core::state::ToolCallStatus;

        // Log session updates to the worker log file AND database
        match &args.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let ContentBlock::Text(text) = &chunk.content {
                    // Log to file (for backwards compatibility)
                    self.log(&text.text);
                    // Write to database for GUI streaming
                    if let Some(state) = self.get_state() {
                        let _ = state.insert_text_event(&self.worker_name, &text.text);
                    }
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                if let ContentBlock::Text(text) = &chunk.content {
                    // Write thought to database
                    if let Some(state) = self.get_state() {
                        let _ = state.insert_thought_event(&self.worker_name, &text.text);
                    }
                }
            }
            SessionUpdate::ToolCall(tc) => {
                // Log to file
                self.log(&format!("\n[tool: {}]", tc.title));

                // Convert ACP status to our status
                let status = match tc.status {
                    agent_client_protocol::ToolCallStatus::Pending => ToolCallStatus::Pending,
                    agent_client_protocol::ToolCallStatus::InProgress => ToolCallStatus::InProgress,
                    agent_client_protocol::ToolCallStatus::Completed => ToolCallStatus::Completed,
                    agent_client_protocol::ToolCallStatus::Failed => ToolCallStatus::Failed,
                    _ => ToolCallStatus::Pending,
                };

                // Convert tool kind to string
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
                let input = tc.raw_input.as_ref()
                    .and_then(|v| serde_json::to_string(v).ok());

                // Write to database
                if let Some(state) = self.get_state() {
                    let _ = state.insert_tool_start_event(
                        &self.worker_name,
                        &tc.tool_call_id.to_string(),
                        &tc.title,
                        kind,
                        status,
                        input.as_deref(),
                    );
                }
            }
            SessionUpdate::ToolCallUpdate(update) => {
                // Convert status if present
                let status = update.fields.status.map(|s| match s {
                    agent_client_protocol::ToolCallStatus::Pending => ToolCallStatus::Pending,
                    agent_client_protocol::ToolCallStatus::InProgress => ToolCallStatus::InProgress,
                    agent_client_protocol::ToolCallStatus::Completed => ToolCallStatus::Completed,
                    agent_client_protocol::ToolCallStatus::Failed => ToolCallStatus::Failed,
                    _ => ToolCallStatus::Pending,
                });

                // Log completion to file
                if let Some(s) = &status {
                    if *s == ToolCallStatus::Completed || *s == ToolCallStatus::Failed {
                        self.log("[/tool]");
                    }
                }

                // Serialize output if present
                let output = update.fields.raw_output.as_ref()
                    .and_then(|v| serde_json::to_string(v).ok());

                // Write to database
                if let Some(state) = self.get_state() {
                    let _ = state.insert_tool_update_event(
                        &self.worker_name,
                        &update.tool_call_id.to_string(),
                        update.fields.title.as_deref(),
                        status,
                        output.as_deref(),
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn read_text_file(&self, args: ReadTextFileRequest) -> Result<ReadTextFileResponse> {
        let path = Path::new(&args.path);
        match std::fs::read_to_string(path) {
            Ok(mut content) => {
                if args.line.is_some() || args.limit.is_some() {
                    let lines: Vec<&str> = content.lines().collect();
                    let start = args.line.map(|l| l as usize).unwrap_or(0);
                    let end = args.limit.map(|l| start + l as usize).unwrap_or(lines.len());
                    content = lines[start.min(lines.len())..end.min(lines.len())].join("\n");
                }
                Ok(ReadTextFileResponse::new(content))
            }
            Err(_) => Err(agent_client_protocol::Error::internal_error()),
        }
    }

    async fn write_text_file(&self, args: WriteTextFileRequest) -> Result<WriteTextFileResponse> {
        let path = Path::new(&args.path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(path, &args.content) {
            Ok(()) => Ok(WriteTextFileResponse::new()),
            Err(_) => Err(agent_client_protocol::Error::internal_error()),
        }
    }

    async fn create_terminal(&self, args: CreateTerminalRequest) -> Result<CreateTerminalResponse> {
        let id = self.terminal_counter.fetch_add(1, Ordering::SeqCst);
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
                let mut terminals = self.terminals.lock().await;
                terminals.insert(terminal_id.clone(), TerminalHandle { child, output: String::new() });
                Ok(CreateTerminalResponse::new(terminal_id))
            }
            Err(_) => Err(agent_client_protocol::Error::internal_error()),
        }
    }

    async fn terminal_output(&self, args: TerminalOutputRequest) -> Result<TerminalOutputResponse> {
        let mut terminals = self.terminals.lock().await;
        if let Some(handle) = terminals.get_mut(&args.terminal_id) {
            let exit_status = handle.child.try_wait().ok().flatten().map(|s| {
                TerminalExitStatus::new().exit_code(s.code().map(|c| c as u32))
            });
            let mut response = TerminalOutputResponse::new(handle.output.clone(), false);
            if let Some(status) = exit_status {
                response = response.exit_status(status);
            }
            Ok(response)
        } else {
            Err(agent_client_protocol::Error::internal_error())
        }
    }

    async fn release_terminal(&self, args: ReleaseTerminalRequest) -> Result<ReleaseTerminalResponse> {
        let mut terminals = self.terminals.lock().await;
        if let Some(mut handle) = terminals.remove(&args.terminal_id) {
            let _ = handle.child.kill().await;
        }
        Ok(ReleaseTerminalResponse::new())
    }

    async fn wait_for_terminal_exit(&self, args: WaitForTerminalExitRequest) -> Result<WaitForTerminalExitResponse> {
        let mut terminals = self.terminals.lock().await;
        if let Some(handle) = terminals.get_mut(&args.terminal_id) {
            match handle.child.wait().await {
                Ok(status) => {
                    let exit_status = TerminalExitStatus::new()
                        .exit_code(status.code().map(|c| c as u32));
                    Ok(WaitForTerminalExitResponse::new(exit_status))
                }
                Err(_) => Err(agent_client_protocol::Error::internal_error()),
            }
        } else {
            Err(agent_client_protocol::Error::internal_error())
        }
    }

    async fn kill_terminal_command(&self, args: KillTerminalCommandRequest) -> Result<KillTerminalCommandResponse> {
        let mut terminals = self.terminals.lock().await;
        if let Some(handle) = terminals.get_mut(&args.terminal_id) {
            let _ = handle.child.kill().await;
        }
        Ok(KillTerminalCommandResponse::new())
    }
}

/// Configuration for running a worker
pub struct WorkerRunConfig {
    pub run_name: String,
    pub worker_name: String,
    pub work_dir: PathBuf,
    pub run_dir: PathBuf,
    pub spec_path: PathBuf,
    pub log_file: PathBuf,
    pub agent_command: Vec<String>,
    pub is_leader: bool,
    pub leader_name: Option<String>,
    pub teammates: Option<Vec<String>>,
    pub resume_session_id: Option<String>,
}

/// Run the ACP worker loop.
/// This spawns the agent process and communicates with it via ACP.
pub async fn run_acp_worker(config: WorkerRunConfig) -> anyhow::Result<()> {
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    info!("[{}] Starting ACP worker for run={}", config.worker_name, config.run_name);

    // Create log file
    if let Some(parent) = config.log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config.log_file, format!(
        "[worker: {}]\n{}\n\n",
        config.worker_name,
        if config.is_leader { "Starting as leader..." } else { "Starting, waiting for tasks..." }
    ))?;

    // Create the client
    let db_path = config.run_dir.join("hirsel.db");
    let client = Arc::new(HirselClient::new(&config.worker_name, &config.log_file, &db_path));

    // Spawn the agent process
    let mut cmd = Command::new(&config.agent_command[0]);
    if config.agent_command.len() > 1 {
        cmd.args(&config.agent_command[1..]);
    }
    cmd.current_dir(&config.work_dir);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());

    // Pass through environment variables
    for (key, value) in collect_agent_env() {
        cmd.env(&key, &value);
    }
    cmd.env("ACP_PERMISSION_MODE", "bypassPermissions");

    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("Failed to get stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("Failed to get stdout"))?;

    info!("[{}] Agent process started, pid={}", config.worker_name, child.id().unwrap_or(0));

    // Convert tokio streams to futures-compatible streams
    let stdin_compat = stdin.compat_write();
    let stdout_compat = stdout.compat();

    // Create the ACP connection
    let (conn, io_task) = ClientSideConnection::new(
        client.clone(),
        stdin_compat,
        stdout_compat,
        |fut| { tokio::task::spawn_local(fut); },
    );

    // Spawn the IO task
    let io_handle = tokio::task::spawn_local(async move {
        if let Err(e) = io_task.await {
            error!("ACP IO error: {:?}", e);
        }
    });

    // Initialize
    let init_request = InitializeRequest::new(ProtocolVersion::LATEST)
        .client_info(Implementation::new("hirsel", env!("CARGO_PKG_VERSION")));
    let init_result = conn.initialize(init_request).await?;
    info!("[{}] ACP initialized: {:?}", config.worker_name, init_result);

    // Create hirsel MCP server config
    let mcp_config = create_hirsel_mcp_config(&config.run_name, &config.worker_name, None);
    let mut mcp_stdio = McpServerStdio::new(&mcp_config.name, &mcp_config.command)
        .args(mcp_config.args);
    if let Some(vars) = mcp_config.env {
        let env_vars: Vec<EnvVariable> = vars.into_iter()
            .map(|v| EnvVariable::new(&v.name, &v.value))
            .collect();
        mcp_stdio = mcp_stdio.env(env_vars);
    }
    let mcp_server = McpServer::Stdio(mcp_stdio);

    // Create new session
    let session_request = NewSessionRequest::new(config.work_dir.to_string_lossy().to_string())
        .mcp_servers(vec![mcp_server]);
    let session = conn.new_session(session_request).await?;
    let session_id = session.session_id;
    info!("[{}] Created session: {}", config.worker_name, session_id);

    // Set bypassPermissions mode
    let mode_request = SetSessionModeRequest::new(session_id.clone(), "bypassPermissions");
    conn.set_session_mode(mode_request).await?;

    // Read the spec
    let spec_content = std::fs::read_to_string(&config.spec_path)
        .unwrap_or_else(|_| "No spec found.".to_string());

    // Build the prompt
    let prompt = build_worker_prompt(
        &config.worker_name,
        &config.run_name,
        &spec_content,
        config.is_leader,
        config.leader_name.as_deref(),
        config.teammates.as_deref(),
        &config.work_dir,
        &config.run_dir,
    );

    // Send the prompt
    info!("[{}] Sending initial prompt ({} chars)", config.worker_name, prompt.len());
    let prompt_request = PromptRequest::new(
        session_id.clone(),
        vec![ContentBlock::Text(TextContent::new(prompt))],
    );
    let result = conn.prompt(prompt_request).await?;

    info!("[{}] Prompt completed: {:?}", config.worker_name, result.stop_reason);

    // Wait for the agent to finish
    drop(conn);
    let _ = io_handle.await;
    let _ = child.wait().await;

    info!("[{}] Worker finished", config.worker_name);
    Ok(())
}

fn build_worker_prompt(
    worker_name: &str,
    run_name: &str,
    spec_content: &str,
    is_leader: bool,
    leader_name: Option<&str>,
    teammates: Option<&[String]>,
    work_dir: &Path,
    run_dir: &Path,
) -> String {
    let is_multi_worker = teammates.map(|t| !t.is_empty()).unwrap_or(false);

    let mut prompt = String::new();

    prompt.push_str("# Hirsel Worker\n\n");
    prompt.push_str(&format!("You are **{}**, an AI coding agent working on a software project.\n\n", worker_name));

    prompt.push_str("## Spec\n\n");
    prompt.push_str(spec_content);
    prompt.push_str("\n\n");

    prompt.push_str("## Your Context\n\n");
    prompt.push_str(&format!("- **Run name:** {}\n", run_name));
    prompt.push_str(&format!("- **Worker name:** {}\n", worker_name));
    prompt.push_str(&format!("- **Run directory:** {}\n", run_dir.display()));
    prompt.push_str(&format!("- **Work directory:** {} (git worktree - write code here)\n\n", work_dir.display()));

    prompt.push_str("## Available MCP Tools\n\n");
    prompt.push_str("You have access to the `hirsel` MCP server with these tools:\n");
    prompt.push_str("- `task_list` - List all tasks\n");
    prompt.push_str("- `task_add(task_id, name)` - Add a new task\n");
    prompt.push_str("- `task_claim(task_id)` - Claim a task to work on\n");
    prompt.push_str("- `task_done(task_id?)` - Mark task done\n");
    prompt.push_str("- `task_unclaim(task_id?)` - Release a task\n");
    prompt.push_str("- `task_await` - Wait for available tasks\n");
    prompt.push_str("- `work_done` - Signal all work is complete\n");
    prompt.push_str("- `msg_send(thread, message, wait?)` - Send a message\n");
    prompt.push_str("- `msg_read(thread?)` - Check for messages\n\n");

    if is_multi_worker {
        let teammates_str = teammates.map(|t| t.join(", ")).unwrap_or_default();
        if is_leader {
            prompt.push_str("## Your Role: LEADER\n\n");
            prompt.push_str(&format!("You are the team leader. Teammates: {}\n\n", teammates_str));
        } else {
            prompt.push_str("## Your Role: TEAM MEMBER\n\n");
            prompt.push_str(&format!("Leader: **{}**. Teammates: {}\n\n", leader_name.unwrap_or("unknown"), teammates_str));
        }
    } else {
        prompt.push_str("## Getting Started\n\n");
        prompt.push_str("1. Use `task_list` to see available tasks\n");
        prompt.push_str("2. Use `task_claim` to claim the first TODO task\n");
        prompt.push_str("3. Work on the task in your work directory\n");
        prompt.push_str("4. Use `task_done` when done\n");
        prompt.push_str("5. Repeat until all tasks are done\n");
        prompt.push_str("6. Call `work_done` to finish\n\n");
    }

    prompt.push_str("Begin by using `task_list` to see available tasks.\n");

    prompt
}
