//! Eval script execution functions.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::core::lifecycle::LocalLifecycleManager;
use crate::core::{Config, EvalStatus, SQLiteState, Status};

use super::types::{EvalConfig, EvalError, EvalResult};

/// Run an eval for a run
pub async fn run_eval(
    run_name: &str,
    eval_name: Option<&str>,
    config: &EvalConfig,
) -> Result<EvalResult, EvalError> {
    let (global_config, _) = Config::load().unwrap_or_else(|e| {
        tracing::warn!("Failed to load config for eval, using defaults: {}", e);
        (Config::default(), vec![])
    });
    let run_dir = global_config.runs_dir().join(run_name);

    if !run_dir.exists() {
        return Err(EvalError::RunNotFound(run_name.to_string()));
    }

    // Check eval script exists
    let script_path = Path::new(&config.script_path);
    if !script_path.exists() {
        return Err(EvalError::ScriptNotFound(config.script_path.clone()));
    }

    let state = SQLiteState::new(run_name).await?;

    // Check if an eval is already running
    if state.get_running_eval().await?.is_some() {
        return Err(EvalError::AlreadyRunning);
    }

    // Set run status to Eval
    state.set_status(Status::Eval).await?;

    // Create log file for this eval
    let logs_dir = run_dir.join("logs");
    fs::create_dir_all(&logs_dir)?;
    let log_file = logs_dir.join(format!("eval_{}.log", eval_name.unwrap_or("unnamed")));

    // Start eval in database
    let eval_id = state
        .start_eval(
            "staging", // branch being evaluated
            eval_name,
            Some(log_file.to_string_lossy().as_ref()),
        )
        .await?;

    // Run the eval script
    let start_time = std::time::Instant::now();
    let result = execute_eval_script(config, &log_file)?;
    let duration_secs = start_time.elapsed().as_secs();

    // Complete the eval in database
    state
        .complete_eval(eval_id, result.passed, &result.feedback)
        .await?;

    // Update run status based on result
    // Create lifecycle manager for worker operations
    let lifecycle = LocalLifecycleManager::new(run_name, run_dir.clone(), vec![])
        .await
        .map_err(|e| {
            EvalError::ProcessFailed(format!("Failed to create lifecycle manager: {}", e))
        })?;

    if result.passed {
        // Kill any remaining worker processes before marking as Done
        if let Err(e) = lifecycle.kill_all_workers().await {
            tracing::warn!("Failed to kill workers after eval passed: {}", e);
        }

        state.set_status(Status::Done).await?;
    } else {
        // Check retry count
        let evals = state.get_evals(100).await?;
        let failed_count = evals
            .iter()
            .filter(|e| e.status == EvalStatus::Failed)
            .count();

        // After 3 failed evals, mark as Failed with EvalFailed reason
        if failed_count >= 3 {
            // Kill any remaining worker processes before marking as Failed
            if let Err(e) = lifecycle.kill_all_workers().await {
                tracing::warn!("Failed to kill workers after eval failures: {}", e);
            }

            state
                .set_failed(crate::core::state::FailureReason::EvalFailed)
                .await?;
        } else {
            // Go back to working for retry
            state.set_status(Status::Working).await?;
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
pub fn execute_eval_script(config: &EvalConfig, log_file: &Path) -> Result<EvalResult, EvalError> {
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
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| EvalError::ProcessFailed("Failed to capture stdout".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| EvalError::ProcessFailed("Failed to capture stderr".to_string()))?;

    let (tx, rx) = mpsc::channel::<(&str, Vec<String>)>();
    let tx_err = tx.clone();

    // Thread to read stdout
    let stdout_handle = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut lines: Vec<String> = Vec::new();
        for line in reader.lines().map_while(Result::ok) {
            lines.push(line);
        }
        let _ = tx.send(("stdout", lines));
    });

    // Thread to read stderr
    let stderr_handle = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let mut lines: Vec<String> = Vec::new();
        for line in reader.lines().map_while(Result::ok) {
            lines.push(line);
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

                let mut stdout_lines: Vec<String> = Vec::new();
                let mut stderr_lines: Vec<String> = Vec::new();

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
                    if let Err(e) = child.kill() {
                        tracing::error!("Failed to kill timed-out eval process: {}", e);
                    }
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
