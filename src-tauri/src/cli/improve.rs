//! Improve command - update project memory from learnings
//!
//! Analyzes learnings from hirsel runs and updates project memory files
//! (CLAUDE.md or AGENTS.md) by spawning an AI agent via ACP to identify
//! patterns and add rules.

use crate::core::{config, state::SQLiteState, Files};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{debug, info, warn};

// ACP imports
use agent_client_protocol::{
    Agent, Client, ClientSideConnection, ContentBlock, CreateTerminalRequest,
    CreateTerminalResponse, Implementation, InitializeRequest, KillTerminalCommandRequest,
    KillTerminalCommandResponse, NewSessionRequest, PermissionOptionKind, PromptRequest,
    ProtocolVersion, ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalRequest,
    ReleaseTerminalResponse, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionNotification, SessionUpdate,
    TerminalOutputRequest, TerminalOutputResponse, TextContent, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse, WriteTextFileRequest, WriteTextFileResponse,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Default timeout for improve agent (120 seconds - it may need to do more work)
const IMPROVE_TIMEOUT_SECS: u64 = 120;

/// Execute the improve command
pub fn execute(run_name: Option<&str>, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    // Create runtime for async execution
    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(async {
        tokio::task::LocalSet::new()
            .run_until(execute_async(run_name, json))
            .await
    });

    // Clean up any remaining child processes (e.g., grandchildren like hirsel __acp-bridge)
    crate::core::process::cleanup_process_group("improve");

    result
}

/// Async implementation of the improve command
async fn execute_async(
    run_name: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // If run_name provided, check it exists
    if let Some(name) = run_name {
        if !config::run_exists(name) {
            if json {
                let output = serde_json::json!({
                    "success": false,
                    "error": "not_found",
                    "message": format!("Run '{}' not found", name),
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("Run '{}' not found", name);
            }
            return Ok(());
        }
    }

    if !json {
        println!("Analyzing learnings...");
    }

    // Collect learnings
    let (learnings, latest_timestamps, project_path) = if let Some(name) = run_name {
        collect_learnings_from_run(name)?
    } else {
        collect_all_learnings()?
    };

    // Check if we have any learnings
    if learnings.is_empty() || learnings.values().all(|msgs| msgs.is_empty()) {
        if json {
            let output = serde_json::json!({
                "success": true,
                "message": "No learnings to process",
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("No learnings to process");
        }
        return Ok(());
    }

    // Check project path
    let project_path = match project_path {
        Some(p) if p.exists() => p,
        Some(p) => {
            if json {
                let output = serde_json::json!({
                    "success": false,
                    "error": "project_not_found",
                    "message": format!("Project path does not exist: {}", p.display()),
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("Project path does not exist: {}", p.display());
            }
            return Ok(());
        }
        None => {
            if json {
                let output = serde_json::json!({
                    "success": false,
                    "error": "no_project",
                    "message": "Could not determine project path",
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("Could not determine project path");
            }
            return Ok(());
        }
    };

    // Detect memory file
    let memory_file = detect_memory_file(&project_path);
    let memory_exists = memory_file.exists();
    let memory_content = if memory_exists {
        std::fs::read_to_string(&memory_file)
            .unwrap_or_else(|_| "(Could not read file)".to_string())
    } else {
        "(File does not exist yet)".to_string()
    };

    // Format learnings for prompt
    let learnings_text = format_learnings(&learnings);

    // Get the improve prompt
    let base_prompt = get_improve_prompt();

    // Build context
    let context = format!(
        r#"
## Learnings to Analyze

{}

## Project Memory File

**Path**: {}
**Exists**: {}

**Current Contents**:
```
{}
```

Analyze the learnings above and update {} with any patterns you find.
"#,
        learnings_text,
        memory_file.display(),
        memory_exists,
        memory_content,
        memory_file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    );

    let full_prompt = format!("{}\n\n{}", base_prompt, context);

    // Get agent command from config
    let (global_config, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));
    let agent_command = &global_config.agent.command;

    // Spawn improve agent via ACP
    let result =
        run_improve_agent_acp(&project_path, &full_prompt, &memory_file, agent_command).await;

    match result {
        Ok(output) => {
            // Update learnings_processed_at timestamps
            for (rn, ts) in &latest_timestamps {
                let run_dir = config::run_dir(rn);
                if run_dir.exists() {
                    let files = Files::new(&run_dir);
                    if let Ok(state) = SQLiteState::new(files.db_path()) {
                        let _ = state.set_learnings_processed_at(ts);
                    }
                }
            }

            if json {
                let output_json = serde_json::json!({
                    "success": true,
                    "message": "Project memory updated",
                    "output": output,
                });
                println!("{}", serde_json::to_string_pretty(&output_json)?);
            } else {
                println!("Project memory updated");
            }
        }
        Err(e) => {
            if json {
                let output = serde_json::json!({
                    "success": false,
                    "error": "agent_failed",
                    "message": e.to_string(),
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("Failed to run improve agent: {}", e);
            }
        }
    }

    Ok(())
}

/// Collect learnings from a specific run
#[allow(clippy::type_complexity)]
fn collect_learnings_from_run(
    run_name: &str,
) -> Result<
    (
        std::collections::HashMap<String, Vec<Learning>>,
        std::collections::HashMap<String, String>,
        Option<PathBuf>,
    ),
    Box<dyn std::error::Error>,
> {
    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path())?;

    let since = state.get_learnings_processed_at().ok().flatten();
    let project_path = state.get_project_path().ok().flatten().map(PathBuf::from);

    let messages = state.get_messages("learnings", 1000)?;

    // Filter by timestamp if needed
    let filtered: Vec<_> = if let Some(ref since_ts) = since {
        messages
            .into_iter()
            .filter(|m| m.timestamp > *since_ts)
            .collect()
    } else {
        messages
    };

    let learnings: Vec<Learning> = filtered
        .into_iter()
        .map(|m| Learning {
            sender: m.sender,
            content: m.content,
            timestamp: m.timestamp,
        })
        .collect();

    let mut result = std::collections::HashMap::new();
    let mut timestamps = std::collections::HashMap::new();

    if !learnings.is_empty() {
        if let Some(latest) = learnings.iter().map(|l| &l.timestamp).max() {
            timestamps.insert(run_name.to_string(), latest.clone());
        }
        result.insert(run_name.to_string(), learnings);
    }

    Ok((result, timestamps, project_path))
}

/// Collect learnings from all runs
#[allow(clippy::type_complexity)]
fn collect_all_learnings() -> Result<
    (
        std::collections::HashMap<String, Vec<Learning>>,
        std::collections::HashMap<String, String>,
        Option<PathBuf>,
    ),
    Box<dyn std::error::Error>,
> {
    let runs_dir = config::runs_dir();
    if !runs_dir.exists() {
        return Ok((
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
        ));
    }

    let mut all_learnings = std::collections::HashMap::new();
    let mut timestamps = std::collections::HashMap::new();
    let mut project_path = None;

    for entry in std::fs::read_dir(&runs_dir)? {
        let entry = entry?;
        let run_dir = entry.path();
        if !run_dir.is_dir() {
            continue;
        }

        let run_name = match run_dir.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        let files = Files::new(&run_dir);
        let db_path = files.db_path();
        if !db_path.exists() {
            continue;
        }

        if let Ok(state) = SQLiteState::new(db_path) {
            let since = state.get_learnings_processed_at().ok().flatten();

            // Get project path from first run that has one
            if project_path.is_none() {
                if let Ok(Some(pp)) = state.get_project_path() {
                    project_path = Some(PathBuf::from(pp));
                }
            }

            if let Ok(messages) = state.get_messages("learnings", 1000) {
                let filtered: Vec<_> = if let Some(ref since_ts) = since {
                    messages
                        .into_iter()
                        .filter(|m| m.timestamp > *since_ts)
                        .collect()
                } else {
                    messages
                };

                let learnings: Vec<Learning> = filtered
                    .into_iter()
                    .map(|m| Learning {
                        sender: m.sender,
                        content: m.content,
                        timestamp: m.timestamp,
                    })
                    .collect();

                if !learnings.is_empty() {
                    if let Some(latest) = learnings.iter().map(|l| &l.timestamp).max() {
                        timestamps.insert(run_name.clone(), latest.clone());
                    }
                    all_learnings.insert(run_name, learnings);
                }
            }
        }
    }

    Ok((all_learnings, timestamps, project_path))
}

/// Learning message
struct Learning {
    sender: String,
    content: String,
    timestamp: String,
}

/// Detect the project memory file (CLAUDE.md or AGENTS.md)
fn detect_memory_file(project_path: &Path) -> PathBuf {
    let claude_md = project_path.join("CLAUDE.md");
    if claude_md.exists() {
        claude_md
    } else {
        // Default to AGENTS.md (whether it exists or not)
        project_path.join("AGENTS.md")
    }
}

/// Format learnings for the prompt
fn format_learnings(learnings: &std::collections::HashMap<String, Vec<Learning>>) -> String {
    if learnings.is_empty() {
        return "(No learnings found)".to_string();
    }

    let mut parts = Vec::new();
    for (run_name, messages) in learnings {
        parts.push(format!("## Run: {}\n", run_name));
        for msg in messages {
            let ts = if msg.timestamp.len() > 16 {
                &msg.timestamp[..16]
            } else {
                &msg.timestamp
            }
            .replace('T', " ");
            parts.push(format!("**{}** ({}):\n{}\n", msg.sender, ts, msg.content));
        }
        parts.push(String::new());
    }

    parts.join("\n")
}

/// Get the improve prompt
fn get_improve_prompt() -> String {
    r#"# Improve Agent

You analyze learnings from hirsel runs and update project memory (CLAUDE.md or AGENTS.md).

IMPORTANT: This task requires you to update the project memory file. You MUST use the file write tool to save your changes. Do NOT just describe the changes - actually write them to the file.

## Your Task

1. Read the learnings messages provided
2. Identify patterns (2+ occurrences = pattern, 3+ = strong pattern)
3. Check existing project memory for rule violations
4. Update project memory with new rules by WRITING to the file

## Process

### Step 1: Analyze Learnings

Look for:
- **Repeated patterns** - Same insight mentioned multiple times
- **User preferences** - Code style, commit format, testing requirements
- **Project-specific knowledge** - Architecture decisions, file locations, gotchas
- **What worked** - Successful approaches worth remembering
- **What didn't work** - Anti-patterns to avoid

Single observations (1 occurrence) are noted but NOT added to memory.

### Step 2: Check for Rule Violations

If the project memory file exists, check if any learnings indicate violations of existing rules.
These get **highest priority** for strengthening.

### Step 3: Update Project Memory

**Format requirements:**
- One line per rule
- Bullet points (- or *)
- Direct, imperative tone ("use X", "avoid Y", "run Z before...")
- NO explanations or rationale in the file
- Group by category if the file has sections

### Step 4: Report Changes

After updating, report:
- Number of patterns identified
- Rules strengthened (if any)
- New rules added
- File updated

## Important

- **Be selective** - Only add rules that will genuinely help future runs
- **Be concise** - One line per rule, no explanations
- **Patterns matter** - Single observations don't become rules
- **Preserve structure** - If the file has sections, maintain them
- **Don't duplicate** - Check existing rules before adding
- **WRITE THE FILE** - Use the file write tool to save changes
"#
    .to_string()
}

// =============================================================================
// ACP Client for Improve Agent
// =============================================================================

/// An ACP client for the improve agent that allows file read/write
/// but denies terminal operations.
struct ImproveClient {
    collected_text: Mutex<String>,
    project_path: PathBuf,
}

impl ImproveClient {
    fn new(project_path: &Path) -> Self {
        Self {
            collected_text: Mutex::new(String::new()),
            project_path: project_path.to_path_buf(),
        }
    }

    async fn get_text(&self) -> String {
        self.collected_text.lock().await.clone()
    }
}

#[async_trait::async_trait(?Send)]
impl Client for ImproveClient {
    async fn request_permission(
        &self,
        args: RequestPermissionRequest,
    ) -> std::result::Result<RequestPermissionResponse, agent_client_protocol::Error> {
        // Auto-approve permissions for improve agent
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
        Ok(RequestPermissionResponse::new(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
        ))
    }

    async fn session_notification(
        &self,
        args: SessionNotification,
    ) -> std::result::Result<(), agent_client_protocol::Error> {
        // Collect text output from agent
        if let SessionUpdate::AgentMessageChunk(chunk) = &args.update {
            if let ContentBlock::Text(text) = &chunk.content {
                let mut collected = self.collected_text.lock().await;
                collected.push_str(&text.text);
            }
        }
        Ok(())
    }

    async fn read_text_file(
        &self,
        args: ReadTextFileRequest,
    ) -> std::result::Result<ReadTextFileResponse, agent_client_protocol::Error> {
        // Allow file reads within the project
        let path = PathBuf::from(&args.path);

        // Security check: only allow reading files within project path
        let canonical_project = self
            .project_path
            .canonicalize()
            .unwrap_or_else(|_| self.project_path.clone());
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());

        if !canonical_path.starts_with(&canonical_project) {
            warn!(
                "Improve agent attempted to read file outside project: {}",
                args.path.display()
            );
            return Err(agent_client_protocol::Error::internal_error());
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(ReadTextFileResponse::new(content)),
            Err(e) => {
                debug!("Failed to read file {}: {}", args.path.display(), e);
                Err(agent_client_protocol::Error::internal_error())
            }
        }
    }

    async fn write_text_file(
        &self,
        args: WriteTextFileRequest,
    ) -> std::result::Result<WriteTextFileResponse, agent_client_protocol::Error> {
        // Allow file writes within the project
        let path = PathBuf::from(&args.path);

        // Security check: only allow writing files within project path
        let canonical_project = self
            .project_path
            .canonicalize()
            .unwrap_or_else(|_| self.project_path.clone());

        // For new files, check parent directory
        let check_path = if path.exists() {
            path.canonicalize().unwrap_or_else(|_| path.clone())
        } else {
            path.parent()
                .and_then(|p| p.canonicalize().ok())
                .unwrap_or_else(|| path.clone())
        };

        if !check_path.starts_with(&canonical_project) {
            warn!(
                "Improve agent attempted to write file outside project: {}",
                args.path.display()
            );
            return Err(agent_client_protocol::Error::internal_error());
        }

        match std::fs::write(&path, &args.content) {
            Ok(()) => {
                info!("Improve agent wrote file: {}", args.path.display());
                Ok(WriteTextFileResponse::new())
            }
            Err(e) => {
                warn!("Failed to write file {}: {}", args.path.display(), e);
                Err(agent_client_protocol::Error::internal_error())
            }
        }
    }

    async fn create_terminal(
        &self,
        _args: CreateTerminalRequest,
    ) -> std::result::Result<CreateTerminalResponse, agent_client_protocol::Error> {
        // Deny terminal creation - improve agent shouldn't need shell access
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn terminal_output(
        &self,
        _args: TerminalOutputRequest,
    ) -> std::result::Result<TerminalOutputResponse, agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn release_terminal(
        &self,
        _args: ReleaseTerminalRequest,
    ) -> std::result::Result<ReleaseTerminalResponse, agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: WaitForTerminalExitRequest,
    ) -> std::result::Result<WaitForTerminalExitResponse, agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn kill_terminal_command(
        &self,
        _args: KillTerminalCommandRequest,
    ) -> std::result::Result<KillTerminalCommandResponse, agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }
}

/// Run the improve agent via ACP
async fn run_improve_agent_acp(
    project_path: &Path,
    prompt: &str,
    memory_file: &Path,
    agent_command: &[String],
) -> Result<String, Box<dyn std::error::Error>> {
    use crate::core::acp::{AcpChild, AcpSpawnConfig};

    if agent_command.is_empty() {
        return Err("Empty agent command".into());
    }

    info!(
        "Running improve agent via ACP to update {}",
        memory_file.display()
    );

    let task = format!(
        "Analyze the learnings and update {}. Report what you changed.",
        memory_file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    );

    // Create improve client
    let client = Arc::new(ImproveClient::new(project_path));

    // Spawn agent process using AcpChild for automatic cleanup
    let spawn_config = AcpSpawnConfig::new(
        agent_command.to_vec(),
        project_path.to_path_buf(),
        "improve",
    );
    let mut acp_child = AcpChild::spawn(spawn_config)?;
    let stdin = acp_child
        .take_stdin()
        .ok_or_else(|| "Failed to get stdin".to_string())?;
    let stdout = acp_child
        .take_stdout()
        .ok_or_else(|| "Failed to get stdout".to_string())?;

    // Convert to futures-compatible streams
    let stdin_compat = stdin.compat_write();
    let stdout_compat = stdout.compat();

    // Create ACP connection
    let (conn, io_task) =
        ClientSideConnection::new(client.clone(), stdin_compat, stdout_compat, |fut| {
            tokio::task::spawn_local(fut);
        });

    // Spawn IO task
    let io_handle = tokio::task::spawn_local(async move {
        if let Err(e) = io_task.await {
            debug!("ACP IO task ended: {:?}", e);
        }
    });

    // Run with timeout
    let result = timeout(Duration::from_secs(IMPROVE_TIMEOUT_SECS), async {
        // Initialize
        let init_request = InitializeRequest::new(ProtocolVersion::LATEST).client_info(
            Implementation::new("hirsel-improve", env!("CARGO_PKG_VERSION")),
        );
        conn.initialize(init_request).await?;

        // Create session with NO MCP servers
        let session_request = NewSessionRequest::new(project_path.to_string_lossy().to_string());
        let session = conn.new_session(session_request).await?;
        let session_id = session.session_id;

        debug!("ACP session created for improve: {}", session_id);

        // Send system prompt first, then the task
        let full_message = format!("{}\n\n---\n\n{}", prompt, task);
        let prompt_request = PromptRequest::new(
            session_id,
            vec![ContentBlock::Text(TextContent::new(full_message))],
        );
        conn.prompt(prompt_request).await?;

        Ok::<(), agent_client_protocol::Error>(())
    })
    .await;

    // Clean up - AcpChild handles process group cleanup on drop
    drop(conn);
    let _ = io_handle.await;
    let _ = acp_child.kill().await;

    // Check result
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            return Err(format!("ACP error: {}", e).into());
        }
        Err(_) => {
            return Err(format!("Timeout after {} seconds", IMPROVE_TIMEOUT_SECS).into());
        }
    }

    // Get collected text output
    let output = client.get_text().await;

    if output.is_empty() {
        return Err("Agent returned no output".into());
    }

    info!("Improve agent completed successfully");

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_memory_file_default() {
        let temp_dir = std::env::temp_dir();
        let result = detect_memory_file(&temp_dir);
        assert!(result.ends_with("AGENTS.md"));
    }

    #[test]
    fn test_format_learnings_empty() {
        let learnings = std::collections::HashMap::new();
        assert_eq!(format_learnings(&learnings), "(No learnings found)");
    }

    #[test]
    fn test_get_improve_prompt() {
        let prompt = get_improve_prompt();
        assert!(prompt.contains("Improve Agent"));
        assert!(prompt.contains("pattern"));
        assert!(prompt.contains("WRITE THE FILE"));
    }
}
