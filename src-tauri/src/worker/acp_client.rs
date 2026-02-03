//! ACP client implementation for hirsel workers.
//!
//! This module implements the Client trait from agent-client-protocol
//! to handle agent requests for permissions, file operations, and terminals.

use agent_client_protocol::{
    Agent, Client, ClientSideConnection, ContentBlock, CreateTerminalRequest,
    CreateTerminalResponse, EnvVariable, Implementation, InitializeRequest,
    KillTerminalCommandRequest, KillTerminalCommandResponse, McpServer, McpServerStdio,
    NewSessionRequest, PermissionOptionKind, PromptRequest, ProtocolVersion, ReadTextFileRequest,
    ReadTextFileResponse, ReleaseTerminalRequest, ReleaseTerminalResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    ResumeSessionRequest, SelectedPermissionOutcome, SessionNotification, SessionUpdate,
    SetSessionModeRequest, TerminalExitStatus, TerminalId, TerminalOutputRequest,
    TerminalOutputResponse, TextContent, WaitForTerminalExitRequest, WaitForTerminalExitResponse,
    WriteTextFileRequest, WriteTextFileResponse,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use crate::core::acp::{create_hirsel_mcp_config, AcpChild, AcpSpawnConfig};

/// Result type for ACP operations
type Result<T> = std::result::Result<T, agent_client_protocol::Error>;

/// Terminal handle for tracking spawned terminals
struct TerminalHandle {
    child: Child,
    output: String,
}

/// Hirsel's implementation of the ACP Client trait.
/// Handles agent requests for permissions, file operations, and terminals.
///
/// This client processes SessionUpdate events from agents (via the ACP protocol)
/// and writes them to the database for streaming UI updates. It's used by both
/// the Node.js ACP adapter and the native Claude CLI bridge.
pub struct HirselClient {
    worker_name: String,
    run_name: String,
    terminals: Mutex<HashMap<TerminalId, TerminalHandle>>,
    terminal_counter: AtomicU64,
}

impl HirselClient {
    /// Get the worker name.
    pub fn worker_name(&self) -> &str {
        &self.worker_name
    }

    /// Get the run name.
    pub fn run_name(&self) -> &str {
        &self.run_name
    }
}

impl HirselClient {
    pub fn new(worker_name: &str, run_name: &str) -> Self {
        Self {
            worker_name: worker_name.to_string(),
            run_name: run_name.to_string(),
            terminals: Mutex::new(HashMap::new()),
            terminal_counter: AtomicU64::new(0),
        }
    }

    async fn get_state(&self) -> Option<crate::core::state::SQLiteState> {
        match crate::core::state::SQLiteState::new(&self.run_name).await {
            Ok(state) => Some(state),
            Err(e) => {
                tracing::warn!(
                    "[{}] Failed to open database for run '{}': {}",
                    self.worker_name,
                    self.run_name,
                    e
                );
                None
            }
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Client for HirselClient {
    async fn request_permission(
        &self,
        args: RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse> {
        // Auto-approve all permission requests (bypassPermissions mode)
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

        debug!("[{}] Auto-approving permission", self.worker_name);

        Ok(RequestPermissionResponse::new(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
        ))
    }

    async fn session_notification(&self, args: SessionNotification) -> Result<()> {
        use crate::core::state::ToolCallStatus;

        // Write session updates to database for streaming
        match &args.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let ContentBlock::Text(text) = &chunk.content {
                    // Write to database for streaming
                    if let Some(state) = self.get_state().await {
                        match state.insert_text_event(&self.worker_name, &text.text).await {
                            Ok(id) => {
                                debug!("[{}] Inserted text event id={}", self.worker_name, id)
                            }
                            Err(e) => {
                                error!("[{}] Failed to insert text event: {}", self.worker_name, e)
                            }
                        }
                    }
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                if let ContentBlock::Text(text) = &chunk.content {
                    // Write thought to database
                    if let Some(state) = self.get_state().await {
                        match state
                            .insert_thought_event(&self.worker_name, &text.text)
                            .await
                        {
                            Ok(id) => {
                                debug!("[{}] Inserted thought event id={}", self.worker_name, id)
                            }
                            Err(e) => error!(
                                "[{}] Failed to insert thought event: {}",
                                self.worker_name, e
                            ),
                        }
                    }
                }
            }
            SessionUpdate::ToolCall(tc) => {
                debug!(
                    "[{}] ToolCall received: id={}, title='{}', status={:?}",
                    self.worker_name, tc.tool_call_id, tc.title, tc.status
                );

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
                let input = tc
                    .raw_input
                    .as_ref()
                    .and_then(|v| serde_json::to_string(v).ok());

                // Write to database
                if let Some(state) = self.get_state().await {
                    match state
                        .insert_tool_start_event(
                            &self.worker_name,
                            &tc.tool_call_id.to_string(),
                            &tc.title,
                            kind,
                            status,
                            input.as_deref(),
                        )
                        .await
                    {
                        Ok(id) => debug!(
                            "[{}] Inserted tool_start event id={} for {}",
                            self.worker_name, id, tc.title
                        ),
                        Err(e) => error!(
                            "[{}] Failed to insert tool_start event for {}: {}",
                            self.worker_name, tc.title, e
                        ),
                    }
                } else {
                    tracing::warn!(
                        "[{}] Dropping ToolCall event - no database connection",
                        self.worker_name
                    );
                }
            }
            SessionUpdate::ToolCallUpdate(update) => {
                debug!(
                    "[{}] ToolCallUpdate received: id={}, title={:?}, status={:?}",
                    self.worker_name,
                    update.tool_call_id,
                    update.fields.title,
                    update.fields.status
                );

                // Convert status if present
                let status = update.fields.status.map(|s| match s {
                    agent_client_protocol::ToolCallStatus::Pending => ToolCallStatus::Pending,
                    agent_client_protocol::ToolCallStatus::InProgress => ToolCallStatus::InProgress,
                    agent_client_protocol::ToolCallStatus::Completed => ToolCallStatus::Completed,
                    agent_client_protocol::ToolCallStatus::Failed => ToolCallStatus::Failed,
                    _ => ToolCallStatus::Pending,
                });

                // Extract output using shared utility
                let output = crate::core::acp::extract_tool_output(&update.fields);

                // Write to database
                if let Some(state) = self.get_state().await {
                    match state
                        .insert_tool_update_event(
                            &self.worker_name,
                            &update.tool_call_id.to_string(),
                            update.fields.title.as_deref(),
                            status,
                            output.as_deref(),
                        )
                        .await
                    {
                        Ok(id) => debug!(
                            "[{}] Inserted tool_update event id={} for {}",
                            self.worker_name, id, update.tool_call_id
                        ),
                        Err(e) => error!(
                            "[{}] Failed to insert tool_update event for {}: {}",
                            self.worker_name, update.tool_call_id, e
                        ),
                    }
                } else {
                    tracing::warn!(
                        "[{}] Dropping ToolCallUpdate event - no database connection",
                        self.worker_name
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
                terminals.insert(
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

    async fn terminal_output(&self, args: TerminalOutputRequest) -> Result<TerminalOutputResponse> {
        let mut terminals = self.terminals.lock().await;
        if let Some(handle) = terminals.get_mut(&args.terminal_id) {
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
    ) -> Result<ReleaseTerminalResponse> {
        let mut terminals = self.terminals.lock().await;
        if let Some(mut handle) = terminals.remove(&args.terminal_id) {
            let _ = handle.child.kill().await;
        }
        Ok(ReleaseTerminalResponse::new())
    }

    async fn wait_for_terminal_exit(
        &self,
        args: WaitForTerminalExitRequest,
    ) -> Result<WaitForTerminalExitResponse> {
        let mut terminals = self.terminals.lock().await;
        if let Some(handle) = terminals.get_mut(&args.terminal_id) {
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
    ) -> Result<KillTerminalCommandResponse> {
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
    pub agent_command: Vec<String>,
    pub is_leader: bool,
    pub leader_name: Option<String>,
    pub teammates: Option<Vec<String>>,
    pub resume_session_id: Option<String>,
    /// Optional API URL for reporting status (used by Docker/remote workers)
    pub api_url: Option<String>,
    /// Task ID assigned to this worker (direct task assignment)
    pub assigned_task_id: Option<String>,
}

/// Run the ACP worker loop.
/// This spawns the agent process and communicates with it via ACP.
pub async fn run_acp_worker(config: WorkerRunConfig) -> anyhow::Result<()> {
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    info!(
        "[{}] Starting ACP worker for run={}",
        config.worker_name, config.run_name
    );

    // Create the client
    let client = Arc::new(HirselClient::new(&config.worker_name, &config.run_name));

    // Spawn the agent process using AcpChild for automatic cleanup
    let spawn_config = AcpSpawnConfig::new(
        config.agent_command.clone(),
        config.work_dir.clone(),
        config.worker_name.clone(),
    );
    let mut acp_child = AcpChild::spawn(spawn_config)?;
    let stdin = acp_child
        .take_stdin()
        .ok_or_else(|| anyhow::anyhow!("Failed to get stdin"))?;
    let stdout = acp_child
        .take_stdout()
        .ok_or_else(|| anyhow::anyhow!("Failed to get stdout"))?;

    // Convert tokio streams to futures-compatible streams
    let stdin_compat = stdin.compat_write();
    let stdout_compat = stdout.compat();

    // Create the ACP connection
    let (conn, io_task) =
        ClientSideConnection::new(client.clone(), stdin_compat, stdout_compat, |fut| {
            tokio::task::spawn_local(fut);
        });

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
    info!(
        "[{}] ACP initialized: {:?}",
        config.worker_name, init_result
    );

    // Create hirsel MCP server config
    // Get the path to the current hirsel binary so MCP server can be spawned
    let hirsel_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()));
    let mcp_config = create_hirsel_mcp_config(
        &config.run_name,
        &config.worker_name,
        hirsel_path.as_deref(),
    );
    let mut mcp_stdio =
        McpServerStdio::new(&mcp_config.name, &mcp_config.command).args(mcp_config.args);
    if let Some(vars) = mcp_config.env {
        let env_vars: Vec<EnvVariable> = vars
            .into_iter()
            .map(|v| EnvVariable::new(&v.name, &v.value))
            .collect();
        mcp_stdio = mcp_stdio.env(env_vars);
    }
    let mcp_server = McpServer::Stdio(mcp_stdio);

    // Create or resume session
    let session_id = if let Some(ref resume_id) = config.resume_session_id {
        // Resume existing session
        info!("[{}] Resuming session: {}", config.worker_name, resume_id);
        let resume_request = ResumeSessionRequest::new(
            resume_id.clone(),
            config.work_dir.to_string_lossy().to_string(),
        )
        .mcp_servers(vec![mcp_server]);
        conn.resume_session(resume_request).await?;
        // When resuming, the session_id is the one we're resuming
        resume_id.clone()
    } else {
        // Create new session
        let session_request = NewSessionRequest::new(config.work_dir.to_string_lossy().to_string())
            .mcp_servers(vec![mcp_server]);
        let session = conn.new_session(session_request).await?;
        session.session_id.to_string()
    };
    info!("[{}] Session ready: {}", config.worker_name, session_id);

    // Set bypassPermissions mode
    let mode_request = SetSessionModeRequest::new(session_id.clone(), "bypassPermissions");
    conn.set_session_mode(mode_request).await?;

    // Build the prompt
    let prompt = build_worker_prompt(
        &config.worker_name,
        &config.run_name,
        config.teammates.as_deref(),
        &config.work_dir,
        &config.run_dir,
        config.assigned_task_id.as_deref(),
    );

    // Send the prompt
    info!(
        "[{}] Sending initial prompt ({} chars)",
        config.worker_name,
        prompt.len()
    );
    let prompt_request = PromptRequest::new(
        session_id.clone(),
        vec![ContentBlock::Text(TextContent::new(prompt))],
    );
    let result = conn.prompt(prompt_request).await?;

    info!(
        "[{}] Prompt completed: {:?}",
        config.worker_name, result.stop_reason
    );

    // Wait for the agent to finish
    drop(conn);
    let _ = io_handle.await;
    let _ = acp_child.wait().await;

    info!("[{}] Worker finished", config.worker_name);

    // AcpChild handles cleanup automatically on drop (kills process group)
    Ok(())
}

/// Build the worker prompt with all context.
///
/// This is used by both the ACP worker and Claude CLI worker implementations.
/// Workers access task details via MCP tools (get_task_tree, get_task_details, etc.)
pub fn build_worker_prompt(
    worker_name: &str,
    run_name: &str,
    teammates: Option<&[String]>,
    work_dir: &Path,
    run_dir: &Path,
    assigned_task_id: Option<&str>,
) -> String {
    let is_multi_worker = teammates.map(|t| !t.is_empty()).unwrap_or(false);

    let mut prompt = String::new();

    // Header
    prompt.push_str("# Hirsel Worker Mode\n\n");
    prompt.push_str("You are an autonomous worker executing a defined task.\n\n");

    // Context
    prompt.push_str("## Your Context\n\n");
    prompt.push_str(&format!("- **Worker name:** {}\n", worker_name));
    prompt.push_str(&format!("- **Run name:** {}\n", run_name));
    if let Some(task_id) = assigned_task_id {
        prompt.push_str(&format!("- **Assigned task:** `{}`\n", task_id));
    }
    prompt.push_str(&format!(
        "- **Work directory:** {} (git worktree - write code here)\n",
        work_dir.display()
    ));
    prompt.push_str(&format!("- **Run directory:** {}\n", run_dir.display()));
    prompt.push_str(&format!(
        "- **Assets directory:** {} (images & files referenced in spec)\n\n",
        run_dir.join("assets").display()
    ));

    // Your Task - Direct assignment
    prompt.push_str("## Your Assigned Task\n\n");
    if let Some(task_id) = assigned_task_id {
        prompt.push_str(&format!("**Task:** `{}`\n\n", task_id));
        prompt.push_str("Use `get_task_details(\"");
        prompt.push_str(task_id);
        prompt.push_str("\")` to see your task content and requirements.\n\n");
    } else {
        prompt.push_str("Check `get_my_tasks()` to see your assigned work.\n\n");
    }
    prompt.push_str("**Task tools:**\n");
    prompt.push_str("- `get_task_details(task_id)` - Get full content for a task\n");
    prompt.push_str("- `get_task_tree()` - See all tasks and their relationships\n");
    prompt.push_str("- `get_available_tasks()` - See tasks ready to work on\n\n");

    // Git workflow
    prompt.push_str("## Git Workflow\n\n");

    if is_multi_worker {
        // Multi-worker: isolated clone with origin pointing to shared staging
        prompt.push_str("You're working on a `staging` branch in your own isolated workspace.\n");
        prompt.push_str("Your `origin` remote points to the shared staging repository.\n\n");
    } else {
        // Single-worker: working directly in staging workspace, no remote
        prompt.push_str("You're working directly on the `staging` branch.\n");
        prompt.push_str("This is a single-worker run - no git remote is configured.\n");
        prompt.push_str("Your changes stay local until the run completes.\n\n");
    }

    prompt.push_str("**Commit Discipline (IMPORTANT):**\n");
    prompt.push_str("- **One commit per logical change** - atomic commits make debugging easy\n");
    prompt.push_str("- **Commit immediately after each change works** - don't batch changes\n");
    prompt.push_str("- **Descriptive messages** - use conventional format: `feat:`, `fix:`, `test:`, `refactor:`\n\n");
    prompt.push_str("```bash\n");
    prompt.push_str("# Good: commit as you go\n");
    prompt.push_str("git add src/auth.py && git commit -m \"feat: add password hashing\"\n");
    prompt.push_str("git add tests/ && git commit -m \"test: add auth unit tests\"\n\n");
    prompt.push_str("# Bad: one commit with everything\n");
    prompt.push_str("git add . && git commit -m \"Add authentication\"  # DON'T DO THIS\n");
    prompt.push_str("```\n\n");

    if is_multi_worker {
        // Multi-worker: must push to share changes
        prompt.push_str("**Before Completing a Task:**\n");
        prompt.push_str(
            "You MUST push your changes to the shared staging before calling `complete_task`:\n",
        );
        prompt.push_str("```bash\n");
        prompt
            .push_str("git add . && git commit -m \"feat: final changes\"  # if any uncommitted\n");
        prompt
            .push_str("git pull origin staging                            # get others' changes\n");
        prompt.push_str("# resolve any conflicts if needed, then:\n");
        prompt.push_str("git push origin staging                            # share your work\n");
        prompt.push_str("```\n");
        prompt.push_str("Only call `complete_task` AFTER your changes are pushed.\n\n");

        prompt.push_str("**Handling Merge Conflicts:**\n");
        prompt.push_str("If `git pull` shows conflicts:\n");
        prompt.push_str(
            "1. Edit conflicted files (remove `<<<<<<<`, `=======`, `>>>>>>>` markers)\n",
        );
        prompt.push_str("2. `git add <resolved-files>`\n");
        prompt.push_str("3. `git commit`\n");
        prompt.push_str("4. `git push origin staging`\n\n");
    } else {
        // Single-worker: just commit, no push needed
        prompt.push_str("**Before Completing a Task:**\n");
        prompt.push_str("Commit any uncommitted changes before calling `complete_task`:\n");
        prompt.push_str("```bash\n");
        prompt.push_str("git add . && git commit -m \"feat: final changes\"\n");
        prompt.push_str("```\n");
        prompt.push_str("No git push is needed - your changes are already in the workspace.\n\n");
    }

    // MCP Tools - IMPORTANT: These are MCP tools, not CLI commands
    prompt.push_str("## Available MCP Tools\n\n");
    prompt.push_str("**IMPORTANT:** You have access to the `hirsel` MCP server. Use these MCP tools directly - do NOT use CLI commands or try to find hirsel binaries.\n\n");

    prompt.push_str("### Task Management\n");
    prompt.push_str("- `get_task_tree()` - Full task hierarchy with status and dependencies\n");
    prompt.push_str("- `get_available_tasks()` - Unblocked, unclaimed tasks ready to work on\n");
    prompt.push_str("- `get_my_tasks()` - Tasks you've claimed\n");
    prompt.push_str("- `get_task_details(task_id)` - Full content for a specific task\n");
    prompt.push_str("- `complete_task(task_id?)` - Mark task done (auto-unblocks dependents)\n");
    prompt.push_str("- `add_task(task_id, name, parent?, blocked_by?)` - Create a new task\n");
    prompt.push_str("  - `task_id`: lowercase with underscores (e.g., `implement_auth`)\n");
    prompt.push_str("  - `parent`: Optional parent task ID for hierarchy\n");
    prompt.push_str("  - `blocked_by`: Array of task IDs that must complete first\n");
    prompt.push_str("- `delete_task(task_id)` - Delete a worker-created task\n");
    prompt.push_str("  - Only tasks you created can be deleted (not spec tasks)\n");
    prompt.push_str("  - Cannot delete claimed or completed tasks\n");
    prompt.push_str("- `add_eval(eval_id, name, validates)` - Create eval task\n");
    prompt.push_str("  - `validates`: Array of task IDs this eval validates\n\n");

    prompt.push_str("### Communication\n");
    prompt
        .push_str("- `list_contacts()` - Available chat targets (user, group, workers, scribe)\n");
    prompt.push_str("- `chat_history(with?, limit?)` - Read message history\n");
    prompt.push_str("  - `with`: Filter by contact ('user', 'group', 'worker-N')\n");
    prompt.push_str("- `chat_send(to, message)` - Send a message\n");
    prompt.push_str("  - Messages to 'user' pause until reply (if HITL enabled)\n");
    prompt.push_str("- `chat_unread(with?)` - Check for new unread messages\n\n");

    prompt.push_str("### Documentation\n");
    prompt.push_str("- `scribe(content)` - Record a learning or discovery\n");
    prompt.push_str("  - Examples: patterns, gotchas, architecture decisions, conventions\n");
    prompt.push_str("  - Batched and integrated into docs/ by a Scribe agent\n");
    prompt.push_str("- `read_docs(file?)` - Read project documentation maintained by Scribe\n");
    prompt.push_str("  - Omit `file` to get all docs, or specify e.g. `patterns.md`\n\n");

    prompt.push_str("### Completion\n");
    prompt.push_str("- `work_done` - Signal task complete and ready for new assignment\n");
    prompt.push_str("  - Auto-completes your currently assigned task\n");
    prompt.push_str("  - You'll exit and be respawned with a new task if available\n");
    prompt.push_str("- `time_status` - Check time limit status\n\n");

    prompt.push_str("### Eval Operations\n");
    prompt.push_str("- `eval_pass()` - Mark eval as passed (only for eval tasks)\n");
    prompt.push_str("- `eval_fail(feedback)` - Mark eval as failed with feedback\n\n");

    // Task statuses
    prompt.push_str("## Task Workflow\n\n");
    prompt.push_str("**Statuses:**\n");
    prompt.push_str("- `TODO` - Available to work on\n");
    prompt.push_str("- `DOING` - Assigned to a worker\n");
    prompt.push_str("- `DONE` - Completed\n");
    prompt.push_str("- `BLOCKED` - Waiting for dependencies\n\n");
    prompt.push_str("**Direct Task Assignment:**\n");
    prompt.push_str("- Your task is **pre-assigned** when you spawn - no need to claim\n");
    prompt.push_str("- Use `get_task_details(task_id)` to see full task content\n");
    prompt.push_str("- Complete the work in your git workspace\n");
    prompt.push_str("- Call `work_done()` when your task is complete\n");
    prompt.push_str("  - This marks your task done and exits\n");
    prompt.push_str("  - You'll be respawned with a new task if one is available\n\n");
    prompt.push_str("**Creating subtasks:**\n");
    prompt.push_str("- You can still use `add_task()` to break down work\n");
    prompt.push_str("- Subtasks go into the pool and may be assigned to you or other workers\n\n");

    // Messaging section
    prompt.push_str("## Messaging the User\n\n");
    prompt.push_str("Messages to `user` pause execution until they reply.\n\n");
    prompt.push_str("**When to message:**\n");
    prompt.push_str("- You need information to proceed\n");
    prompt.push_str("- Significant architectural decision\n");
    prompt.push_str("- Spec is ambiguous\n");
    prompt.push_str("- Something the user should review\n\n");
    prompt.push_str("```\n");
    prompt.push_str(
        "msg_send(\"user\", \"Which database should I use - PostgreSQL or SQLite?\", wait=true)\n",
    );
    prompt.push_str("```\n\n");

    // Documentation
    prompt.push_str("## Project Documentation\n\n");
    prompt
        .push_str("Use `scribe(content)` to record discoveries. A Scribe agent maintains docs/:\n");
    prompt.push_str("- `architecture.md` - System design, module relationships\n");
    prompt.push_str("- `patterns.md` - Code patterns and conventions\n");
    prompt.push_str("- `gotchas.md` - Pitfalls and things to watch out for\n");
    prompt.push_str("- `decisions.md` - Key decisions and rationale\n\n");
    prompt.push_str("**At task start:** Call `read_docs()` to check accumulated knowledge.\n");
    prompt.push_str("**During work:** Call `scribe()` when you discover something useful.\n\n");

    // Getting started
    prompt.push_str("## Getting Started\n\n");
    prompt.push_str("1. Use `get_task_details(your_assigned_task)` to see your task\n");
    prompt.push_str("2. Work on the task in your git workspace\n");
    prompt.push_str("3. Commit your changes\n");
    prompt.push_str("4. Call `work_done()` - your task is auto-completed and you'll be respawned with a new task if available\n\n");

    // When stuck
    prompt.push_str("## When Stuck\n\n");
    prompt.push_str("Don't spin. If you can't figure something out after 2-3 attempts:\n");
    prompt.push_str("```\n");
    prompt.push_str("chat_send(\"user\", \"Specific question about what's blocking you\")\n");
    prompt.push_str("```\n\n");

    prompt.push_str("**Begin by reviewing your assigned task with `get_task_details()`.**\n");

    prompt
}

/// Run a worker using the ACP protocol.
///
/// All workers now use `run_acp_worker` which communicates via the ACP protocol
/// using `ClientSideConnection`. This gives us proper tool title handling -
/// the ACP protocol populates `tc.title` correctly, fixing the "Tool" label
/// issue in the spectate view.
pub async fn run_worker(config: WorkerRunConfig) -> anyhow::Result<()> {
    run_acp_worker(config).await
}
