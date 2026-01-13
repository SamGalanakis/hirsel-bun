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
