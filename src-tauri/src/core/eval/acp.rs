//! ACP-based evaluation agent.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::core::lifecycle::LocalLifecycleManager;
use crate::core::{EvalStatus, Files, SQLiteState, Status};

use super::context::{build_eval_context, build_eval_prompt, copy_dir_all};
use super::types::{EvalAcpConfig, EvalAcpResult, EvalError};

/// Maximum number of times to prompt the agent if it doesn't submit a verdict.
const MAX_VERDICT_RETRIES: usize = 2;

/// Reminder prompt sent if agent doesn't submit verdict.
const VERDICT_REMINDER_PROMPT: &str = r#"You have not yet submitted your evaluation verdict.

You MUST call one of these MCP tools to complete your evaluation:

- `mcp__eval__eval_pass` - if all checks passed
- `mcp__eval__eval_fail` - if any check failed (include feedback parameter)

Please call the appropriate tool NOW to submit your verdict."#;

/// Clean up a test run using the standard delete logic
fn cleanup_test_run(run_name: &str) {
    use tracing::{info, warn};

    // Small delay to allow any pending writes to complete
    std::thread::sleep(std::time::Duration::from_millis(500));

    match crate::cli::delete::execute(run_name, false) {
        Ok(()) => info!("[cleanup] Deleted test run '{}'", run_name),
        Err(e) => warn!("[cleanup] Failed to delete test run '{}': {}", run_name, e),
    }
}

/// Entry point for the `__eval-run` CLI command.
///
/// This function is called by the background subprocess spawned by the lifecycle manager.
/// It sets up the eval configuration and runs the ACP-based eval agent.
pub async fn run_eval_from_args(
    run_name: &str,
    run_dir: &str,
    agent_command_json: &str,
) -> Result<(), EvalError> {
    use tracing::info;

    let run_dir = PathBuf::from(run_dir);
    let files = Files::new(&run_dir);

    // Parse agent command
    let agent_command: Vec<String> = serde_json::from_str(agent_command_json)
        .map_err(|e| EvalError::ProcessFailed(format!("Invalid agent command JSON: {}", e)))?;

    // Get or create eval record in database
    let state = SQLiteState::new(files.db_path())?;

    // Create eval name with detective/QA themed naming
    let existing_evals = state.get_evals(1000)?;
    let eval_number = existing_evals.len() + 1;
    let eval_name = crate::core::names::generate_eval_name(eval_number);

    // Create logs directory
    let logs_dir = run_dir.join("logs");
    fs::create_dir_all(&logs_dir)?;

    // Create log and result files
    let log_file = logs_dir.join(format!("{}.log", eval_name));
    let result_file = run_dir
        .join("tmp")
        .join(format!("{}_result.json", eval_name));
    fs::create_dir_all(run_dir.join("tmp"))?;

    // Create isolated eval worktree - full copy including untracked files
    let eval_work_dir = run_dir.join("work").join(&eval_name);
    if eval_work_dir.exists() {
        fs::remove_dir_all(&eval_work_dir)?;
    }
    let staging_dir = run_dir.join("work").join("staging");
    copy_dir_all(&staging_dir, &eval_work_dir)?;

    // Remove git remote so eval cannot push changes (but can still view history and run hooks)
    match std::process::Command::new("git")
        .args(["remote", "remove", "origin"])
        .current_dir(&eval_work_dir)
        .output()
    {
        Ok(output) if !output.status.success() => {
            tracing::warn!(
                "[{}] Failed to remove git remote (exit {}): {}",
                eval_name,
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Err(e) => {
            tracing::warn!("[{}] Failed to run git remote remove: {}", eval_name, e);
        }
        _ => {}
    }

    info!(
        "[{}] Created isolated eval worktree at {:?}",
        eval_name, eval_work_dir
    );

    // Start eval in database
    let eval_id = state.start_eval(
        "staging",
        Some(&eval_name),
        Some(log_file.to_string_lossy().as_ref()),
    )?;

    // Store our PID so the eval can be killed when pausing
    let pid = std::process::id();
    state.set_eval_pid(eval_id, pid)?;

    info!(
        "[{}] Starting eval (id={}, pid={}) for run {}",
        eval_name, eval_id, pid, run_name
    );

    // Build config for ACP eval
    let config = EvalAcpConfig {
        eval_name: eval_name.clone(),
        eval_id,
        work_dir: eval_work_dir.clone(),
        run_dir: run_dir.clone(),
        result_file,
        log_file,
        timeout_secs: 600, // 10 minute timeout
        agent_command,
    };

    // Run the eval
    let result = run_eval_acp(config).await;

    // Cleanup eval worktree
    if let Err(e) = fs::remove_dir_all(&eval_work_dir) {
        tracing::warn!("[{}] Failed to cleanup eval worktree: {}", eval_name, e);
    }

    let result = result?;

    // Update eval record with result
    state.complete_eval(eval_id, result.success, &result.feedback)?;

    // Create lifecycle manager for worker operations
    let (cfg, _) = crate::core::Config::load().unwrap_or_else(|e| {
        tracing::warn!(
            "[{}] Failed to load config, using defaults: {}",
            eval_name,
            e
        );
        (crate::core::Config::default(), vec![])
    });
    let agent_cmd = cfg.agent.command.clone();
    let lifecycle =
        LocalLifecycleManager::new(run_name, run_dir.clone(), agent_cmd).map_err(|e| {
            EvalError::ProcessFailed(format!("Failed to create lifecycle manager: {}", e))
        })?;

    // Update run status based on result
    if result.success {
        // Kill any remaining worker processes before marking as Done
        let killed = lifecycle.kill_all_workers().unwrap_or_else(|e| {
            tracing::warn!("[{}] Failed to kill workers: {}", eval_name, e);
            vec![]
        });
        if !killed.is_empty() {
            info!(
                "[{}] Killed {} remaining worker(s)",
                eval_name,
                killed.len()
            );
        }

        info!("[{}] Eval PASSED - marking run as Done", eval_name);
        state.set_status(Status::Done)?;

        // Auto-cleanup test runs
        if state.is_test_run().unwrap_or(false) {
            info!(
                "[{}] Test run completed successfully - cleaning up",
                eval_name
            );
            cleanup_test_run(run_name);
            return Ok(());
        }
    } else {
        // Check retry count
        let evals = state.get_evals(100)?;
        let failed_count = evals
            .iter()
            .filter(|e| e.status == EvalStatus::Failed)
            .count();

        if failed_count >= 3 {
            // Kill any remaining worker processes before marking as Failed
            let killed = lifecycle.kill_all_workers().unwrap_or_else(|e| {
                tracing::warn!("[{}] Failed to kill workers: {}", eval_name, e);
                vec![]
            });
            if !killed.is_empty() {
                info!(
                    "[{}] Killed {} remaining worker(s)",
                    eval_name,
                    killed.len()
                );
            }

            info!(
                "[{}] Eval FAILED ({} failures) - marking run as Failed",
                eval_name, failed_count
            );
            state.set_failed(crate::core::state::FailureReason::EvalFailed)?;

            // Auto-cleanup test runs
            if state.is_test_run().unwrap_or(false) {
                info!("[{}] Test run failed - cleaning up", eval_name);
                cleanup_test_run(run_name);
                return Ok(());
            }
        } else {
            info!(
                "[{}] Eval FAILED ({} failures) - resuming workers for retry",
                eval_name, failed_count
            );

            // Add feedback as a remediation task
            let task_id = format!("eval_fix_{}", failed_count);
            let task_desc = format!("Fix eval failures:\n\n{}", result.feedback);
            state.add_task(&task_id, &task_desc, None, None)?;

            // Set back to Working status to resume workers
            state.set_status(Status::Working)?;

            // Resume awaiting workers
            if let Err(e) = lifecycle.resume_awaiting_workers() {
                tracing::error!(
                    "[{}] Failed to resume workers after eval failure: {}",
                    eval_name,
                    e
                );
            }
        }
    }

    // Clean up any remaining child processes (e.g., grandchildren like hirsel __acp-bridge)
    crate::core::process::cleanup_process_group(&eval_name);

    Ok(())
}

/// Run an evaluation using an ACP agent.
///
/// This spawns an AI agent with an eval MCP server that provides eval_pass/eval_fail
/// tools. The agent is sent the eval prompt and is expected to call one of these
/// tools to submit its verdict.
pub async fn run_eval_acp(config: EvalAcpConfig) -> Result<EvalAcpResult, EvalError> {
    use crate::core::acp::{AcpChild, AcpSpawnConfig};
    use agent_client_protocol::{
        Agent, ClientSideConnection, ContentBlock, EnvVariable, Implementation, InitializeRequest,
        McpServer, McpServerStdio, NewSessionRequest, PromptRequest, ProtocolVersion,
        SetSessionModeRequest, TextContent,
    };
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
    use tracing::{error, info};

    // Build context from run state
    let files = Files::new(&config.run_dir);
    let state = SQLiteState::new(files.db_path())?;
    let ctx = build_eval_context(&files, &state);

    // Build the full prompt from context
    let full_prompt = build_eval_prompt(&ctx);

    // Write log file header
    fs::write(
        &config.log_file,
        format!(
            "# Eval {} (id={})\n# Started: {}\n\n",
            config.eval_name,
            config.eval_id,
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S")
        ),
    )?;

    // Get the hirsel executable for eval MCP server
    let hirsel_exe = std::env::current_exe()
        .map_err(|e| EvalError::ProcessFailed(format!("Failed to get current exe: {}", e)))?;

    // Create MCP server config for eval
    let mcp_env = vec![EnvVariable::new(
        "HIRSEL_EVAL_RESULT_FILE",
        config.result_file.to_string_lossy().as_ref(),
    )];

    let mcp_stdio = McpServerStdio::new("eval", hirsel_exe.to_string_lossy().as_ref())
        .args(vec!["__eval-mcp".to_string()])
        .env(mcp_env);
    let mcp_server = McpServer::Stdio(mcp_stdio);

    // Spawn the agent process using AcpChild for automatic cleanup
    if config.agent_command.is_empty() {
        return Err(EvalError::ProcessFailed(
            "Agent command is empty".to_string(),
        ));
    }

    // Resolve "hirsel" to current executable path (it may not be in PATH)
    let mut resolved_command = config.agent_command.clone();
    if let Some(first) = resolved_command.first_mut() {
        if first == "hirsel" || first.ends_with("/hirsel") {
            if let Ok(exe) = std::env::current_exe() {
                *first = exe.to_string_lossy().into_owned();
            }
        }
    }

    let spawn_config = AcpSpawnConfig::new(
        resolved_command,
        config.work_dir.clone(),
        config.eval_name.clone(),
    );
    let mut acp_child = AcpChild::spawn(spawn_config)
        .map_err(|e| EvalError::ProcessFailed(format!("Failed to spawn agent: {}", e)))?;

    let stdin = acp_child
        .take_stdin()
        .ok_or_else(|| EvalError::ProcessFailed("Failed to get stdin".to_string()))?;
    let stdout = acp_child
        .take_stdout()
        .ok_or_else(|| EvalError::ProcessFailed("Failed to get stdout".to_string()))?;

    // Convert tokio streams to futures-compatible streams
    let stdin_compat = stdin.compat_write();
    let stdout_compat = stdout.compat();

    // Create the client
    let db_path = config.run_dir.join("hirsel.db");
    let client = Arc::new(crate::worker::acp_client::HirselClient::new(
        &config.eval_name,
        &db_path,
    ));

    // Create ACP connection
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
    let init_request = InitializeRequest::new(ProtocolVersion::LATEST).client_info(
        Implementation::new("hirsel-eval", env!("CARGO_PKG_VERSION")),
    );

    conn.initialize(init_request)
        .await
        .map_err(|e| EvalError::ProcessFailed(format!("ACP initialize failed: {}", e)))?;

    // Create session with MCP server
    let session_request = NewSessionRequest::new(config.work_dir.to_string_lossy().to_string())
        .mcp_servers(vec![mcp_server]);

    let session = conn
        .new_session(session_request)
        .await
        .map_err(|e| EvalError::ProcessFailed(format!("Failed to create session: {}", e)))?;

    let session_id = session.session_id;
    info!(
        "[{}] Created eval session: {}",
        config.eval_name, session_id
    );

    // Set to bypass permissions mode
    let mode_request = SetSessionModeRequest::new(session_id.clone(), "bypassPermissions");
    conn.set_session_mode(mode_request)
        .await
        .map_err(|e| EvalError::ProcessFailed(format!("Failed to set mode: {}", e)))?;

    // Prompt loop with retries
    let mut current_prompt = full_prompt;
    let mut attempt = 0;

    loop {
        // Build prompt content
        let prompt_content = vec![ContentBlock::Text(TextContent::new(current_prompt.clone()))];
        let prompt_request = PromptRequest::new(session_id.clone(), prompt_content);

        let timeout = tokio::time::Duration::from_secs(config.timeout_secs);
        match tokio::time::timeout(timeout, conn.prompt(prompt_request)).await {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                if let Err(kill_err) = acp_child.kill().await {
                    tracing::warn!(
                        "[{}] Failed to kill eval agent: {}",
                        config.eval_name,
                        kill_err
                    );
                }
                io_handle.abort();
                return Err(EvalError::ProcessFailed(format!("Prompt failed: {}", e)));
            }
            Err(_) => {
                if let Err(kill_err) = acp_child.kill().await {
                    tracing::warn!(
                        "[{}] Failed to kill eval agent on timeout: {}",
                        config.eval_name,
                        kill_err
                    );
                }
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
            if let Err(kill_err) = acp_child.kill().await {
                tracing::warn!(
                    "[{}] Failed to kill eval agent after max retries: {}",
                    config.eval_name,
                    kill_err
                );
            }
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
    let result_content = fs::read_to_string(&config.result_file).map_err(EvalError::Io)?;

    #[derive(serde::Deserialize)]
    struct ResultFile {
        success: bool,
        feedback: String,
    }

    let result: ResultFile = serde_json::from_str(&result_content)
        .map_err(|e| EvalError::ProcessFailed(format!("Invalid result file: {}", e)))?;

    // Kill agent process and cleanup
    if let Err(kill_err) = acp_child.kill().await {
        tracing::warn!(
            "[{}] Failed to kill eval agent on completion: {}",
            config.eval_name,
            kill_err
        );
    }
    io_handle.abort();

    info!(
        "[{}] Eval completed: success={}",
        config.eval_name, result.success
    );

    Ok(EvalAcpResult {
        success: result.success,
        feedback: result.feedback,
        eval_id: config.eval_id,
        eval_name: config.eval_name,
    })
}
