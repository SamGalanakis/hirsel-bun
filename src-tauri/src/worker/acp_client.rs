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
    db_path: PathBuf,
    terminals: Mutex<HashMap<TerminalId, TerminalHandle>>,
    terminal_counter: AtomicU64,
}

impl HirselClient {
    /// Get the worker name.
    pub fn worker_name(&self) -> &str {
        &self.worker_name
    }

    /// Get the database path.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }
}

impl HirselClient {
    pub fn new(worker_name: &str, db_path: &Path) -> Self {
        Self {
            worker_name: worker_name.to_string(),
            db_path: db_path.to_path_buf(),
            terminals: Mutex::new(HashMap::new()),
            terminal_counter: AtomicU64::new(0),
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

                // Extract output using shared utility
                let output = crate::core::acp::extract_tool_output(&update.fields);

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
    pub spec_path: PathBuf,
    pub agent_command: Vec<String>,
    pub is_leader: bool,
    pub leader_name: Option<String>,
    pub teammates: Option<Vec<String>>,
    pub resume_session_id: Option<String>,
    /// Optional API URL for reporting status (used by Docker/remote workers)
    pub api_url: Option<String>,
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
    let db_path = config.run_dir.join("hirsel.db");
    let client = Arc::new(HirselClient::new(&config.worker_name, &db_path));

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

    // Read the spec
    let spec_content =
        std::fs::read_to_string(&config.spec_path).unwrap_or_else(|_| "No spec found.".to_string());

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
#[allow(clippy::too_many_arguments)]
pub fn build_worker_prompt(
    worker_name: &str,
    run_name: &str,
    _spec_content: &str, // No longer embedded - workers use MCP tools to access tasks
    is_leader: bool,
    leader_name: Option<&str>,
    teammates: Option<&[String]>,
    work_dir: &Path,
    run_dir: &Path,
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
    prompt.push_str(&format!(
        "- **Work directory:** {} (git worktree - write code here)\n",
        work_dir.display()
    ));
    prompt.push_str(&format!("- **Run directory:** {}\n", run_dir.display()));
    prompt.push_str(&format!(
        "- **Assets directory:** {} (images & files referenced in spec)\n\n",
        run_dir.join("assets").display()
    ));

    // Your Task - MCP-first approach
    prompt.push_str("## Your Task\n\n");
    prompt.push_str("You have a pre-defined task scope. Use MCP tools to understand the work:\n\n");
    prompt.push_str("1. `get_task_tree()` - See all tasks and their relationships\n");
    prompt.push_str("2. `get_task_details(id)` - Get full content for a specific task\n");
    prompt.push_str("3. `get_available_tasks()` - See what's ready to work on\n\n");
    prompt.push_str(
        "Tasks were created from a planning board - use these tools to understand the scope.\n\n",
    );

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
    prompt.push_str("- `claim_task(task_id)` - Claim a task (TODO → DOING)\n");
    prompt.push_str("- `complete_task(task_id?)` - Mark task done (auto-unblocks dependents)\n");
    prompt.push_str("- `add_task(task_id, name, parent?, blocked_by?)` - Create a new task\n");
    prompt.push_str("  - `task_id`: lowercase with underscores (e.g., `implement_auth`)\n");
    prompt.push_str("  - `parent`: Optional parent task ID for hierarchy\n");
    prompt.push_str("  - `blocked_by`: Array of task IDs that must complete first\n");
    prompt.push_str("- `add_eval(eval_id, name, validates)` - Create verification task\n");
    prompt.push_str("  - `validates`: Array of task IDs this eval verifies\n\n");

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
    prompt.push_str("- `work_done` - Signal all work is complete (triggers verification)\n");
    prompt.push_str("- `time_status` - Check time limit status\n\n");

    prompt.push_str("### Eval Operations\n");
    prompt.push_str("- `eval_pass()` - Mark eval as passed (only for eval tasks)\n");
    prompt.push_str("- `eval_fail(feedback)` - Mark eval as failed with feedback\n\n");

    // Task statuses
    prompt.push_str("## Task Workflow\n\n");
    prompt.push_str("**Statuses:**\n");
    prompt.push_str("- `TODO` - Available to claim\n");
    prompt.push_str("- `DOING` - Claimed by a worker\n");
    prompt.push_str("- `DONE` - Completed\n");
    prompt.push_str("- `BLOCKED` - Waiting for dependencies\n\n");
    prompt.push_str("**Rules:**\n");
    prompt.push_str(
        "- **NEVER edit code without a claimed task** - if no task exists, create one first\n",
    );
    prompt.push_str("- You can only have **one claimed task** at a time\n");
    prompt.push_str("- You can only complete tasks you have claimed\n");
    prompt.push_str("- If you need to switch tasks, release your current one first\n\n");

    // The scope task
    prompt.push_str("## The \"scope\" Task\n\n");
    prompt.push_str("You start with a \"scope\" task already claimed. Review the task tree and decide your approach:\n\n");

    prompt.push_str("**1. Explore first** - If unfamiliar with codebase:\n");
    prompt.push_str("   - Create exploration tasks to understand the code\n");
    prompt.push_str("   - Use `scribe()` to record findings\n");
    prompt.push_str("   - Create implementation tasks after exploration\n\n");

    prompt.push_str("**2. Plan more** - If tasks need breakdown:\n");
    prompt.push_str("   - Create subtasks for large tasks\n");
    prompt.push_str("   - Add blocking relationships where needed\n\n");

    prompt.push_str("**3. Start directly** - If tasks are well-defined:\n");
    prompt.push_str("   - Complete the scope task to unblock other tasks\n");
    prompt.push_str("   - Begin working on available tasks\n\n");

    prompt.push_str("When you complete the scope task, blocked tasks become available.\n\n");

    // Task design principles
    prompt.push_str("## Task Design Principles\n\n");
    prompt.push_str("**Parallel execution:**\n");
    prompt.push_str("- Minimize dependencies between tasks\n");
    prompt.push_str("- Prefer vertical slices (complete features) over horizontal layers\n");
    prompt.push_str("- Tasks touching same files = conflicts. Structure to minimize overlap.\n\n");
    prompt.push_str("**Task ordering:**\n");
    prompt.push_str("- Tackle unknowns (spikes) before mechanical work\n");
    prompt.push_str("- A failed spike might restructure the whole plan\n\n");
    prompt.push_str("**Dependencies (blocked_by):**\n");
    prompt.push_str("When in doubt, add the dependency. Better slow than broken:\n");
    prompt.push_str("- Task reads files another writes? → Add dependency\n");
    prompt.push_str("- Task calls functions another creates? → Add dependency\n");
    prompt.push_str("- Task tests code another implements? → Add dependency\n\n");

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

    // Role-specific section
    if is_multi_worker {
        let teammates_str = teammates.map(|t| t.join(", ")).unwrap_or_default();
        if is_leader {
            prompt.push_str("## Your Role: LEADER\n\n");
            prompt.push_str(&format!(
                "You are the team leader. Teammates: {}\n\n",
                teammates_str
            ));
            prompt.push_str("**Your responsibilities:**\n");
            prompt
                .push_str("- Claim the `scope` task and create exploration/implementation tasks\n");
            prompt.push_str("- Design tasks to minimize conflicts (different files per task)\n");
            prompt.push_str("- Use `group` thread to coordinate with teammates\n");
            prompt.push_str("- Announce major decisions in group chat\n\n");
        } else {
            prompt.push_str("## Your Role: TEAM MEMBER\n\n");
            prompt.push_str(&format!(
                "Leader: **{}**. Teammates: {}\n\n",
                leader_name.unwrap_or("unknown"),
                teammates_str
            ));
            prompt.push_str("**Your workflow:**\n");
            prompt.push_str("1. Check `chat_unread()` for team updates\n");
            prompt.push_str("2. If no tasks available, wait for leader to complete scoping\n");
            prompt.push_str("3. **Announce intent in `group` before claiming** ambiguous tasks\n");
            prompt.push_str("4. Claim task, work on it, push, mark done\n\n");
            prompt.push_str("**Do NOT call `work_done` just because no tasks yet** - leader may still be scoping.\n\n");
        }

        prompt.push_str("## Group Chat Coordination\n\n");
        prompt.push_str("Use `chat_send(\"group\", ...)` to coordinate with teammates:\n\n");
        prompt.push_str("**When to message the group:**\n");
        prompt.push_str("- Before claiming ambiguous tasks (announce intent)\n");
        prompt.push_str("- When changing shared code (utils, models, configs)\n");
        prompt.push_str("- When discovering patterns others should follow\n");
        prompt.push_str("- When changing interfaces (function signatures, schemas)\n");
        prompt.push_str("- When finding surprises or gotchas\n\n");
        prompt.push_str("**Examples:**\n");
        prompt.push_str("```\n");
        prompt.push_str("chat_send(\"group\", \"I'm taking auth_setup - will use JWT tokens\")\n");
        prompt.push_str("chat_send(\"group\", \"Changed User model - added 'role' field\")\n");
        prompt.push_str("chat_send(\"group\", \"FYI: tests require REDIS_URL env var\")\n");
        prompt.push_str("```\n\n");
    }

    // Getting started
    prompt.push_str("## Getting Started\n\n");
    prompt.push_str("1. Use `get_task_tree()` to see all tasks and relationships\n");
    prompt.push_str("2. If scope task is yours (claimed), review tasks and decide approach\n");
    prompt.push_str("3. Complete scope task to unblock other tasks\n");
    prompt.push_str("4. Use `get_available_tasks()` to find work\n");
    prompt.push_str("5. `claim_task(id)` → work on it → commit → `complete_task()`\n");
    prompt.push_str("6. Repeat until all tasks done\n");
    prompt.push_str("7. Call `work_done()` to finish\n\n");

    // When stuck
    prompt.push_str("## When Stuck\n\n");
    prompt.push_str("Don't spin. If you can't figure something out after 2-3 attempts:\n");
    prompt.push_str("```\n");
    prompt.push_str("chat_send(\"user\", \"Specific question about what's blocking you\")\n");
    prompt.push_str("```\n\n");

    prompt.push_str("**Begin by using `get_task_tree()` to see available tasks.**\n");

    prompt
}

// =============================================================================
// Claude CLI Worker (native Rust, no Node.js dependency)
// =============================================================================

/// Run a worker using the native Claude CLI bridge.
///
/// This is an alternative to `run_acp_worker` that communicates directly with
/// the Claude CLI using its JSON streaming protocol.
pub async fn run_claude_cli_worker(config: WorkerRunConfig) -> anyhow::Result<()> {
    use crate::core::claude_cli::{run_claude_worker, ClaudeWorkerConfig};

    info!(
        "[{}] Starting Claude CLI worker for run={}",
        config.worker_name, config.run_name
    );

    // Read the spec
    let spec_content =
        std::fs::read_to_string(&config.spec_path).unwrap_or_else(|_| "No spec found.".to_string());

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

    // Create Claude worker config
    let worker_config = ClaudeWorkerConfig {
        run_name: config.run_name,
        worker_name: config.worker_name,
        work_dir: config.work_dir,
        run_dir: config.run_dir,
        prompt,
        resume_session_id: config.resume_session_id,
    };

    // Run the worker
    let result = run_claude_worker(worker_config).await?;

    info!(
        "Worker completed: stop_reason={:?}, tokens={:?}/{:?}, cost=${:?}",
        result.stop_reason, result.input_tokens, result.output_tokens, result.cost_usd
    );

    Ok(())
}

/// Run a worker, automatically selecting the best backend.
///
/// If the agent command indicates Claude or our built-in ACP bridge, uses the
/// native Claude Agent SDK. Otherwise falls back to the ACP adapter for external
/// agents.
pub async fn run_worker(config: WorkerRunConfig) -> anyhow::Result<()> {
    // Check if we should use the native Claude Agent SDK
    // This includes:
    // - Empty command (default to Claude)
    // - "claude" or path ending in "/claude"
    // - "hirsel __acp-bridge" (our built-in bridge, now uses SDK)
    let use_sdk = config.agent_command.is_empty()
        || config.agent_command.first().is_some_and(|cmd| {
            cmd == "claude" || cmd.ends_with("/claude") || cmd.contains("claude-code")
        })
        || config.agent_command.iter().any(|arg| arg == "__acp-bridge");

    if use_sdk {
        return run_claude_cli_worker(config).await;
    }

    // Fall back to ACP adapter for external agents
    run_acp_worker(config).await
}
