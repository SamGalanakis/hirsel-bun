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
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, SetSessionModeRequest,
    TerminalExitStatus, TerminalId, TerminalOutputRequest, TerminalOutputResponse, TextContent,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse, WriteTextFileRequest,
    WriteTextFileResponse,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, error, info};

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

                // Log completion to file
                if let Some(s) = &status {
                    if *s == ToolCallStatus::Completed || *s == ToolCallStatus::Failed {
                        self.log("[/tool]");
                    }
                }

                // Serialize output if present
                let output = update
                    .fields
                    .raw_output
                    .as_ref()
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

    info!(
        "[{}] Starting ACP worker for run={}",
        config.worker_name, config.run_name
    );

    // Create log file
    if let Some(parent) = config.log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        &config.log_file,
        format!(
            "[worker: {}]\n{}\n\n",
            config.worker_name,
            if config.is_leader {
                "Starting as leader..."
            } else {
                "Starting, waiting for tasks..."
            }
        ),
    )?;

    // Create the client
    let db_path = config.run_dir.join("hirsel.db");
    let client = Arc::new(HirselClient::new(
        &config.worker_name,
        &config.log_file,
        &db_path,
    ));

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
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to get stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to get stdout"))?;

    info!(
        "[{}] Agent process started, pid={}",
        config.worker_name,
        child.id().unwrap_or(0)
    );

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
    prompt.push_str(&format!("- **Run directory:** {}\n\n", run_dir.display()));

    // Spec
    prompt.push_str("## Spec\n\n");
    prompt.push_str(spec_content);
    prompt.push_str("\n\n");

    // Git workflow
    prompt.push_str("## Git Workflow\n\n");
    prompt.push_str("You're working on a `staging` branch in your own isolated workspace.\n");
    prompt.push_str("Your `origin` remote points to the shared staging repository.\n\n");

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

    prompt.push_str("**Before Completing a Task:**\n");
    prompt
        .push_str("You MUST push your changes to the shared staging before calling `task_done`:\n");
    prompt.push_str("```bash\n");
    prompt.push_str("git add . && git commit -m \"feat: final changes\"  # if any uncommitted\n");
    prompt.push_str("git pull origin staging                            # get others' changes\n");
    prompt.push_str("# resolve any conflicts if needed, then:\n");
    prompt.push_str("git push origin staging                            # share your work\n");
    prompt.push_str("```\n");
    prompt.push_str("Only call `task_done` AFTER your changes are pushed.\n\n");

    prompt.push_str("**Handling Merge Conflicts:**\n");
    prompt.push_str("If `git pull` shows conflicts:\n");
    prompt.push_str("1. Edit conflicted files (remove `<<<<<<<`, `=======`, `>>>>>>>` markers)\n");
    prompt.push_str("2. `git add <resolved-files>`\n");
    prompt.push_str("3. `git commit`\n");
    prompt.push_str("4. `git push origin staging`\n\n");

    // MCP Tools - IMPORTANT: These are MCP tools, not CLI commands
    prompt.push_str("## Available MCP Tools\n\n");
    prompt.push_str("**IMPORTANT:** You have access to the `hirsel` MCP server. Use these MCP tools directly - do NOT use CLI commands or try to find hirsel binaries.\n\n");
    prompt.push_str("### Task Management\n");
    prompt.push_str("- `task_list` - Show all tasks\n");
    prompt.push_str("- `task_add(task_id, name, parent?, blocked_by?)` - Add a new task\n");
    prompt.push_str("  - `task_id`: lowercase with underscores (e.g., `implement_auth`)\n");
    prompt.push_str("  - `parent`: Optional parent task ID for hierarchy\n");
    prompt.push_str("  - `blocked_by`: Array of task IDs that must complete first\n");
    prompt.push_str("- `task_claim(task_id)` - Claim a task (TODO → DOING)\n");
    prompt.push_str("- `task_done(task_id?)` - Complete current task (DOING → DONE)\n");
    prompt.push_str("- `task_unclaim(task_id?)` - Release without completing\n");
    prompt.push_str("- `task_delete(task_id)` - Delete a task and its children\n");
    prompt.push_str("- `task_await` - Wait for tasks to become available\n\n");
    prompt.push_str("### Messaging\n");
    prompt.push_str("- `msg_send(thread, message, wait?)` - Send a message\n");
    prompt
        .push_str("  - Threads: `user` (human), `learnings` (shared knowledge), `group` (team)\n");
    prompt.push_str("  - Set `wait: true` to pause until reply (auto for `user` thread)\n");
    prompt.push_str("- `msg_read(thread?)` - Read all unread messages\n");
    prompt.push_str("- `msg_inbox` - Quick check for new messages this session\n");
    prompt.push_str("- `msg_list` - List available threads\n\n");
    prompt.push_str("### Completion\n");
    prompt.push_str("- `work_done` - Signal all work is complete (triggers verification)\n");
    prompt.push_str("- `time_status` - Check time limit status\n\n");

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
    prompt.push_str("- If you need to switch tasks, `task_unclaim` your current one first\n\n");

    // The scope task and three-phase workflow
    prompt.push_str("## The \"scope\" Task - Three-Phase Workflow\n\n");
    prompt.push_str("Most runs start with a single task: `scope`. This is NOT where you create implementation tasks.\n\n");

    prompt.push_str("### Phase 1: Scoping (the \"scope\" task)\n\n");
    prompt.push_str("1. Claim the `scope` task\n");
    prompt.push_str("2. Read the spec above to understand what needs to be built\n");
    prompt.push_str("3. **Quickly scan the codebase** - get a high-level sense of structure\n");
    prompt.push_str("4. **Create exploration tasks** - one per area needing investigation:\n");
    prompt.push_str("   ```\n");
    prompt.push_str("   task_add(\"explore_existing\", \"Explore existing code structure. Post findings to learnings.\")\n");
    prompt.push_str(
        "   task_add(\"explore_tests\", \"Explore test patterns. Post findings to learnings.\")\n",
    );
    prompt.push_str("   ```\n");
    prompt.push_str("5. **Create implementation planning task** blocked by exploration:\n");
    prompt.push_str("   ```\n");
    prompt.push_str("   task_add(\"create_plan\", \"Create implementation tasks from findings\", blocked_by=[\"explore_existing\", \"explore_tests\"])\n");
    prompt.push_str("   ```\n");
    prompt.push_str("6. Complete the `scope` task\n\n");

    prompt.push_str("### Phase 2: Exploration\n\n");
    prompt.push_str("For each exploration task:\n");
    prompt.push_str("1. Deep-dive into that area\n");
    prompt.push_str("2. **Document findings in learnings chat:**\n");
    prompt.push_str("   ```\n");
    prompt.push_str("   msg_send(\"learnings\", \"AUTH: Uses JWT tokens in src/auth/jwt.py\")\n");
    prompt.push_str("   msg_send(\"learnings\", \"TESTS: pytest with fixtures in conftest.py\")\n");
    prompt.push_str("   ```\n");
    prompt.push_str("3. Complete the task\n\n");

    prompt.push_str("### Phase 3: Implementation Planning\n\n");
    prompt.push_str("1. Read all learnings: `msg_read(\"learnings\")`\n");
    prompt.push_str("2. Create concrete implementation tasks with full context\n");
    prompt.push_str("3. Apply task design principles (see below)\n");
    prompt.push_str("4. Complete the planning task\n\n");

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

    // Learnings
    prompt.push_str("## Learnings Chat\n\n");
    prompt.push_str("The `learnings` thread is for recording discoveries:\n");
    prompt.push_str("- Patterns found in the codebase\n");
    prompt.push_str("- Gotchas and surprises\n");
    prompt.push_str("- Architecture decisions\n\n");
    prompt.push_str("Keep entries concise - one sentence per message.\n\n");

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
            prompt.push_str("1. Check `msg_inbox` for team updates\n");
            prompt.push_str("2. If no tasks available, use `task_await` to wait for leader\n");
            prompt.push_str("3. **Announce intent in `group` before claiming** ambiguous tasks\n");
            prompt.push_str("4. Claim task, work on it, push, mark done\n\n");
            prompt.push_str("**Do NOT call `work_done` just because no tasks yet** - leader may still be scoping.\n\n");
        }

        prompt.push_str("## Group Chat Coordination\n\n");
        prompt.push_str("Use `msg_send(\"group\", ...)` to coordinate with teammates:\n\n");
        prompt.push_str("**When to message the group:**\n");
        prompt.push_str("- Before claiming ambiguous tasks (announce intent)\n");
        prompt.push_str("- When changing shared code (utils, models, configs)\n");
        prompt.push_str("- When discovering patterns others should follow\n");
        prompt.push_str("- When changing interfaces (function signatures, schemas)\n");
        prompt.push_str("- When finding surprises or gotchas\n\n");
        prompt.push_str("**Examples:**\n");
        prompt.push_str("```\n");
        prompt.push_str("msg_send(\"group\", \"I'm taking auth_setup - will use JWT tokens\")\n");
        prompt.push_str("msg_send(\"group\", \"Changed User model - added 'role' field\")\n");
        prompt.push_str("msg_send(\"group\", \"FYI: tests require REDIS_URL env var\")\n");
        prompt.push_str("```\n\n");
    }

    // Getting started
    prompt.push_str("## Getting Started\n\n");
    prompt.push_str("1. Use `task_list` to see available tasks\n");
    prompt.push_str("2. If you see a `scope` task, claim it and follow the three-phase workflow\n");
    prompt.push_str("3. Otherwise, claim the next TODO task\n");
    prompt.push_str("4. Work on the task, commit frequently\n");
    prompt.push_str("5. Use `task_done` when complete\n");
    prompt.push_str("6. Repeat until all tasks done\n");
    prompt.push_str("7. Call `work_done` to finish\n\n");

    // When stuck
    prompt.push_str("## When Stuck\n\n");
    prompt.push_str("Don't spin. If you can't figure something out after 2-3 attempts:\n");
    prompt.push_str("```\n");
    prompt.push_str(
        "msg_send(\"user\", \"Specific question about what's blocking you\", wait=true)\n",
    );
    prompt.push_str("```\n\n");

    prompt.push_str("**Begin by using `task_list` to see available tasks.**\n");

    prompt
}
