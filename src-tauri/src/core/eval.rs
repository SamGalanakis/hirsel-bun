//! Eval system for running automated checks on completed work.
//!
//! The eval system runs scripts to verify work quality, capturing feedback
//! and handling retry logic for failed evals.

use crate::core::{Config, EvalStatus, SQLiteState, Status};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use thiserror::Error;

/// Eval-related errors
#[derive(Error, Debug)]
pub enum EvalError {
    #[error("Run not found: {0}")]
    RunNotFound(String),

    #[error("No eval script configured")]
    NoEvalScript,

    #[error("Eval script not found: {0}")]
    ScriptNotFound(String),

    #[error("Eval already running")]
    AlreadyRunning,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("State error: {0}")]
    State(#[from] crate::core::state::StateError),

    #[error("Timeout after {0} seconds")]
    Timeout(u32),

    #[error("Process failed: {0}")]
    ProcessFailed(String),
}

/// Result of running an eval
#[derive(Debug, Clone)]
pub struct EvalResult {
    pub passed: bool,
    pub feedback: String,
    pub duration_secs: u64,
    pub exit_code: Option<i32>,
}

/// Configuration for running an eval
#[derive(Debug, Clone)]
pub struct EvalConfig {
    pub script_path: String,
    pub work_dir: String,
    pub timeout_secs: u32,
    pub env: Vec<(String, String)>,
}

/// Run an eval for a run
pub fn run_eval(
    run_name: &str,
    eval_name: Option<&str>,
    config: &EvalConfig,
) -> Result<EvalResult, EvalError> {
    let (global_config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));
    let run_dir = global_config.runs_dir().join(run_name);

    if !run_dir.exists() {
        return Err(EvalError::RunNotFound(run_name.to_string()));
    }

    // Check eval script exists
    let script_path = Path::new(&config.script_path);
    if !script_path.exists() {
        return Err(EvalError::ScriptNotFound(config.script_path.clone()));
    }

    let db_path = run_dir.join("hirsel.db");
    let state = SQLiteState::new(db_path)?;

    // Check if an eval is already running
    if state.get_running_eval()?.is_some() {
        return Err(EvalError::AlreadyRunning);
    }

    // Set run status to Eval
    state.set_status(Status::Eval)?;

    // Create log file for this eval
    let logs_dir = run_dir.join("logs");
    fs::create_dir_all(&logs_dir)?;
    let log_file = logs_dir.join(format!(
        "eval_{}.log",
        eval_name.unwrap_or("unnamed")
    ));

    // Start eval in database
    let eval_id = state.start_eval(
        "staging", // branch being evaluated
        eval_name,
        Some(log_file.to_string_lossy().as_ref()),
    )?;

    // Run the eval script
    let start_time = std::time::Instant::now();
    let result = execute_eval_script(config, &log_file)?;
    let duration_secs = start_time.elapsed().as_secs();

    // Complete the eval in database
    state.complete_eval(eval_id, result.passed, &result.feedback)?;

    // Update run status based on result
    if result.passed {
        state.set_status(Status::Done)?;
    } else {
        // Check retry count
        let evals = state.get_evals(100)?;
        let failed_count = evals
            .iter()
            .filter(|e| e.status == EvalStatus::Failed)
            .count();

        // After 3 failed evals, mark as EvalFailed
        if failed_count >= 3 {
            state.set_status(Status::EvalFailed)?;
        } else {
            // Go back to working for retry
            state.set_status(Status::Working)?;
        }
    }

    Ok(EvalResult {
        passed: result.passed,
        feedback: result.feedback,
        duration_secs,
        exit_code: result.exit_code,
    })
}

/// Execute the eval script and capture output
fn execute_eval_script(
    config: &EvalConfig,
    log_file: &Path,
) -> Result<EvalResult, EvalError> {
    let script_path = Path::new(&config.script_path);

    // Determine how to run the script
    let (cmd, args) = if script_path.extension().map(|e| e == "sh").unwrap_or(false) {
        ("bash", vec![config.script_path.clone()])
    } else if script_path.extension().map(|e| e == "py").unwrap_or(false) {
        ("python3", vec![config.script_path.clone()])
    } else {
        // Try to run directly (executable script)
        (&config.script_path[..], vec![])
    };

    let mut command = Command::new(cmd);
    for arg in &args {
        command.arg(arg);
    }

    command
        .current_dir(&config.work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Add environment variables
    for (key, value) in &config.env {
        command.env(key, value);
    }

    let mut child = command.spawn()?;

    // Capture output with timeout
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let (tx, rx) = mpsc::channel();
    let tx_err = tx.clone();

    // Thread to read stdout
    let stdout_handle = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut lines = Vec::new();
        for line in reader.lines() {
            if let Ok(line) = line {
                lines.push(line);
            }
        }
        let _ = tx.send(("stdout", lines));
    });

    // Thread to read stderr
    let stderr_handle = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let mut lines = Vec::new();
        for line in reader.lines() {
            if let Ok(line) = line {
                lines.push(line);
            }
        }
        let _ = tx_err.send(("stderr", lines));
    });

    // Wait for process with timeout
    let timeout = Duration::from_secs(config.timeout_secs as u64);
    let start = std::time::Instant::now();

    loop {
        match child.try_wait()? {
            Some(status) => {
                // Process finished
                let _ = stdout_handle.join();
                let _ = stderr_handle.join();

                let mut stdout_lines = Vec::new();
                let mut stderr_lines = Vec::new();

                // Collect output from channels
                while let Ok((stream, lines)) = rx.try_recv() {
                    match stream {
                        "stdout" => stdout_lines = lines,
                        "stderr" => stderr_lines = lines,
                        _ => {}
                    }
                }

                // Write to log file
                let mut log_content = String::new();
                log_content.push_str("=== STDOUT ===\n");
                for line in &stdout_lines {
                    log_content.push_str(line);
                    log_content.push('\n');
                }
                log_content.push_str("\n=== STDERR ===\n");
                for line in &stderr_lines {
                    log_content.push_str(line);
                    log_content.push('\n');
                }
                fs::write(log_file, &log_content)?;

                // Determine pass/fail
                let passed = status.success();
                let exit_code = status.code();

                // Generate feedback from output
                let feedback = if passed {
                    "Eval passed".to_string()
                } else {
                    // Take last few lines of stderr or stdout as feedback
                    let error_lines: Vec<_> = if !stderr_lines.is_empty() {
                        stderr_lines.iter().rev().take(10).rev().cloned().collect()
                    } else {
                        stdout_lines.iter().rev().take(10).rev().cloned().collect()
                    };
                    error_lines.join("\n")
                };

                return Ok(EvalResult {
                    passed,
                    feedback,
                    duration_secs: start.elapsed().as_secs(),
                    exit_code,
                });
            }
            None => {
                // Still running, check timeout
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err(EvalError::Timeout(config.timeout_secs));
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Run eval in a tmux session for interactive viewing
pub fn run_eval_in_tmux(
    run_name: &str,
    _eval_name: Option<&str>,
    config: &EvalConfig,
) -> Result<String, EvalError> {
    let session_name = format!("hirsel-{}-eval", run_name);

    // Check if session already exists
    let exists = Command::new("tmux")
        .args(["has-session", "-t", &session_name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if exists {
        // Kill existing session
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .status();
    }

    // Create new tmux session running the eval
    let script_cmd = if Path::new(&config.script_path)
        .extension()
        .map(|e| e == "sh")
        .unwrap_or(false)
    {
        format!("bash {}", config.script_path)
    } else if Path::new(&config.script_path)
        .extension()
        .map(|e| e == "py")
        .unwrap_or(false)
    {
        format!("python3 {}", config.script_path)
    } else {
        config.script_path.clone()
    };

    let status = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            &session_name,
            "-c",
            &config.work_dir,
            &script_cmd,
        ])
        .status()?;

    if !status.success() {
        return Err(EvalError::ProcessFailed(
            "Failed to create tmux session".to_string(),
        ));
    }

    Ok(session_name)
}

/// Parse eval script content to find test commands
pub fn parse_eval_script(content: &str) -> Vec<String> {
    // Look for common test command patterns
    let mut commands = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Common test commands
        if line.starts_with("pytest")
            || line.starts_with("npm test")
            || line.starts_with("cargo test")
            || line.starts_with("go test")
            || line.starts_with("make test")
            || line.starts_with("./")
        {
            commands.push(line.to_string());
        }
    }

    commands
}

/// Check if eval script exists for a run
pub fn has_eval_script(run_dir: &Path) -> bool {
    let eval_md = run_dir.join("eval.md");
    let eval_sh = run_dir.join("eval.sh");
    eval_md.exists() || eval_sh.exists()
}

/// Get eval script path for a run
pub fn get_eval_script(run_dir: &Path) -> Option<String> {
    let eval_sh = run_dir.join("eval.sh");
    if eval_sh.exists() {
        return Some(eval_sh.to_string_lossy().to_string());
    }

    let eval_md = run_dir.join("eval.md");
    if eval_md.exists() {
        // Extract script from markdown - look for code blocks
        if let Ok(content) = fs::read_to_string(&eval_md) {
            // Look for ```bash or ```sh code blocks
            let mut in_code_block = false;
            let mut script_lines = Vec::new();

            for line in content.lines() {
                if line.starts_with("```bash") || line.starts_with("```sh") {
                    in_code_block = true;
                    continue;
                }
                if line.starts_with("```") && in_code_block {
                    break;
                }
                if in_code_block {
                    script_lines.push(line);
                }
            }

            if !script_lines.is_empty() {
                // Write extracted script to temp file
                let temp_script = run_dir.join("tmp").join("eval_extracted.sh");
                if let Ok(()) = fs::create_dir_all(run_dir.join("tmp")) {
                    if let Ok(()) = fs::write(&temp_script, script_lines.join("\n")) {
                        return Some(temp_script.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    None
}

// =============================================================================
// ACP-Based Evaluation
// =============================================================================

/// Maximum number of times to prompt the agent if it doesn't submit a verdict.
const MAX_VERDICT_RETRIES: usize = 2;

/// Reminder prompt sent if agent doesn't submit verdict.
const VERDICT_REMINDER_PROMPT: &str = r#"You have not yet submitted your evaluation verdict.

You MUST call one of these MCP tools to complete your evaluation:

- `mcp__eval__eval_pass` - if all checks passed
- `mcp__eval__eval_fail` - if any check failed (include feedback parameter)

Please call the appropriate tool NOW to submit your verdict."#;

/// Get the eval prompt template.
fn get_eval_prompt() -> String {
    r#"# hirsel Eval Mode

You are an eval agent verifying work done by other agents.

## CRITICAL: You MUST Submit a Verdict

Your evaluation is NOT complete until you call one of these MCP tools:

- **`mcp__eval__eval_pass`** - Call if all checks pass. No parameters needed.
- **`mcp__eval__eval_fail`** - Call if any check fails. Requires `feedback` parameter.

Writing text output is NOT enough. You MUST call one of these tools to submit your verdict. If you don't call a tool, your evaluation will be marked as failed.

## Process

1. Read the eval specification below
2. Examine the code in the current directory
3. Run any checks specified (tests, startup, file existence, etc.)
4. **Call `mcp__eval__eval_pass` or `mcp__eval__eval_fail` to submit your verdict**

## Guidelines

- Be thorough but focused on the spec
- Don't modify any code - you are read-only
- If a check is ambiguous, fail with clear explanation
- Be specific in your feedback about what failed and how to fix it

## Feedback Format (for eval_fail)

```
Checks:
- [PASS] Server starts on port 8000
- [PASS] /healthz returns {"status": "ok"}
- [FAIL] POST /api/vote returns 500 error

To fix: The vote handler references undefined variable `user_id`. Change line 45 to use `current_user.id` instead.
```

Remember: Call `mcp__eval__eval_pass` or `mcp__eval__eval_fail` when done!
"#.to_string()
}

/// Configuration for running an ACP-based eval.
#[derive(Debug, Clone)]
pub struct EvalAcpConfig {
    pub run_name: String,
    pub eval_name: String,
    pub eval_id: i64,
    pub spec: String,
    pub eval_spec: String,
    pub work_dir: std::path::PathBuf,
    pub run_dir: std::path::PathBuf,
    pub result_file: std::path::PathBuf,
    pub log_file: std::path::PathBuf,
    pub timeout_secs: u64,
    pub agent_command: Vec<String>,
}

/// Result of an ACP-based eval.
#[derive(Debug, Clone)]
pub struct EvalAcpResult {
    pub success: bool,
    pub feedback: String,
    pub eval_id: i64,
    pub eval_name: String,
}

/// Run an evaluation using an ACP agent.
///
/// This spawns an AI agent with an eval MCP server that provides eval_pass/eval_fail
/// tools. The agent is sent the eval prompt and is expected to call one of these
/// tools to submit its verdict.
pub async fn run_eval_acp(config: EvalAcpConfig) -> Result<EvalAcpResult, EvalError> {
    use agent_client_protocol::{
        Agent, ClientSideConnection, InitializeRequest, NewSessionRequest,
        PromptRequest, SetSessionModeRequest, McpServer, McpServerStdio,
        Implementation, ContentBlock, TextContent, ProtocolVersion, EnvVariable,
    };
    use tokio::process::Command;
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
    use std::sync::Arc;
    use tracing::{info, error};

    // Build the full prompt
    let base_prompt = get_eval_prompt();
    let time_context = ""; // TODO: Add time context if needed
    let full_prompt = format!(
        "{}\n\n## Run\n{}\n\n## Eval Name\n{}\n{}\n## Original Spec (what workers were asked to build)\n{}\n\n## Eval Specification (what to verify)\n{}\n\n## Work Directory\n{}\n\nYou are positioned in the project directory. Evaluate the code according to the specifications above. Use the eval tools to submit your verdict.",
        base_prompt,
        config.run_name,
        config.eval_name,
        time_context,
        config.spec,
        config.eval_spec,
        config.work_dir.display()
    );

    // Write log file header
    fs::write(&config.log_file, format!(
        "# Eval {} (id={})\n# Started: {}\n\n",
        config.eval_name,
        config.eval_id,
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S")
    ))?;

    // Get the hirsel executable for eval MCP server
    let hirsel_exe = std::env::current_exe()
        .map_err(|e| EvalError::ProcessFailed(format!("Failed to get current exe: {}", e)))?;

    // Create MCP server config for eval
    let mcp_env = vec![
        EnvVariable::new("HIRSEL_EVAL_RESULT_FILE", config.result_file.to_string_lossy().as_ref()),
    ];

    let mcp_stdio = McpServerStdio::new("eval", hirsel_exe.to_string_lossy().as_ref())
        .args(vec!["__eval-mcp".to_string()])
        .env(mcp_env);
    let mcp_server = McpServer::Stdio(mcp_stdio);

    // Spawn the agent process
    if config.agent_command.is_empty() {
        return Err(EvalError::ProcessFailed("Agent command is empty".to_string()));
    }

    let mut cmd = Command::new(&config.agent_command[0]);
    if config.agent_command.len() > 1 {
        cmd.args(&config.agent_command[1..]);
    }

    cmd.current_dir(&config.work_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .env("ACP_PERMISSION_MODE", "bypassPermissions");

    let mut child = cmd.spawn()
        .map_err(|e| EvalError::ProcessFailed(format!("Failed to spawn agent: {}", e)))?;

    let stdin = child.stdin.take()
        .ok_or_else(|| EvalError::ProcessFailed("Failed to get stdin".to_string()))?;
    let stdout = child.stdout.take()
        .ok_or_else(|| EvalError::ProcessFailed("Failed to get stdout".to_string()))?;

    info!("[{}] Eval agent process started, pid={}", config.eval_name, child.id().unwrap_or(0));

    // Convert tokio streams to futures-compatible streams
    let stdin_compat = stdin.compat_write();
    let stdout_compat = stdout.compat();

    // Create the client
    let db_path = config.run_dir.join("hirsel.db");
    let client = Arc::new(crate::worker::acp_client::HirselClient::new(&config.eval_name, &config.log_file, &db_path));

    // Create ACP connection
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
        .client_info(Implementation::new("hirsel-eval", env!("CARGO_PKG_VERSION")));

    conn.initialize(init_request).await
        .map_err(|e| EvalError::ProcessFailed(format!("ACP initialize failed: {}", e)))?;

    // Create session with MCP server
    let session_request = NewSessionRequest::new(config.work_dir.to_string_lossy().to_string())
        .mcp_servers(vec![mcp_server]);

    let session = conn.new_session(session_request).await
        .map_err(|e| EvalError::ProcessFailed(format!("Failed to create session: {}", e)))?;

    let session_id = session.session_id;
    info!("[{}] Created eval session: {}", config.eval_name, session_id);

    // Set to bypass permissions mode
    let mode_request = SetSessionModeRequest::new(session_id.clone(), "bypassPermissions");
    conn.set_session_mode(mode_request).await
        .map_err(|e| EvalError::ProcessFailed(format!("Failed to set mode: {}", e)))?;

    // Prompt loop with retries
    let mut current_prompt = full_prompt;
    let mut attempt = 0;

    loop {
        // Build prompt content
        let prompt_content = vec![
            ContentBlock::Text(TextContent::new(current_prompt.clone()))
        ];
        let prompt_request = PromptRequest::new(session_id.clone(), prompt_content);

        let timeout = tokio::time::Duration::from_secs(config.timeout_secs);
        match tokio::time::timeout(timeout, conn.prompt(prompt_request)).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                let _ = child.kill().await;
                io_handle.abort();
                return Err(EvalError::ProcessFailed(format!("Prompt failed: {}", e)));
            }
            Err(_) => {
                let _ = child.kill().await;
                io_handle.abort();
                let timeout_mins = config.timeout_secs / 60;
                return Ok(EvalAcpResult {
                    success: false,
                    feedback: format!("Eval timed out after {} minutes", timeout_mins),
                    eval_id: config.eval_id,
                    eval_name: config.eval_name,
                });
            }
        }

        // Check if result file exists
        if config.result_file.exists() {
            break;
        }

        // Retry logic
        attempt += 1;
        if attempt > MAX_VERDICT_RETRIES {
            let _ = child.kill().await;
            io_handle.abort();
            return Ok(EvalAcpResult {
                success: false,
                feedback: "Agent did not submit verdict after multiple prompts".to_string(),
                eval_id: config.eval_id,
                eval_name: config.eval_name,
            });
        }

        current_prompt = VERDICT_REMINDER_PROMPT.to_string();
    }

    // Read result file
    let result_content = fs::read_to_string(&config.result_file)
        .map_err(|e| EvalError::Io(e))?;

    #[derive(serde::Deserialize)]
    struct ResultFile {
        success: bool,
        feedback: String,
    }

    let result: ResultFile = serde_json::from_str(&result_content)
        .map_err(|e| EvalError::ProcessFailed(format!("Invalid result file: {}", e)))?;

    // Kill agent process and cleanup
    let _ = child.kill().await;
    io_handle.abort();

    info!("[{}] Eval completed: success={}", config.eval_name, result.success);

    Ok(EvalAcpResult {
        success: result.success,
        feedback: result.feedback,
        eval_id: config.eval_id,
        eval_name: config.eval_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_eval_script() {
        let script = r#"
#!/bin/bash
# Run tests
pytest tests/
npm test
cargo test
"#;
        let commands = parse_eval_script(script);
        assert_eq!(commands.len(), 3);
        assert!(commands[0].starts_with("pytest"));
        assert!(commands[1].starts_with("npm test"));
        assert!(commands[2].starts_with("cargo test"));
    }

    #[test]
    fn test_has_eval_script() {
        let temp = TempDir::new().unwrap();
        assert!(!has_eval_script(temp.path()));

        // Create eval.md
        fs::write(temp.path().join("eval.md"), "# Eval").unwrap();
        assert!(has_eval_script(temp.path()));
    }

    #[test]
    fn test_get_eval_script_from_sh() {
        let temp = TempDir::new().unwrap();
        let script_path = temp.path().join("eval.sh");
        fs::write(&script_path, "#!/bin/bash\npytest").unwrap();

        let result = get_eval_script(temp.path());
        assert!(result.is_some());
        assert!(result.unwrap().contains("eval.sh"));
    }

    #[test]
    fn test_get_eval_script_from_md() {
        let temp = TempDir::new().unwrap();
        let md_content = r#"# Eval

Run the tests:

```bash
pytest tests/
```
"#;
        fs::write(temp.path().join("eval.md"), md_content).unwrap();

        let result = get_eval_script(temp.path());
        assert!(result.is_some());
    }
}
