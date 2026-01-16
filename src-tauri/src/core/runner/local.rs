//! Local runner implementation - spawns workers as local processes.
//!
//! This runner spawns worker processes on the local machine using the
//! hirsel __worker-run command as a detached subprocess.

use async_trait::async_trait;
use std::collections::HashMap;
use std::process::{Command, Stdio};
use tracing::{debug, info, warn};

use super::{Runner, RunnerError, RunnerResult, SpawnResult, WorkerHandle, WorkerSpawnConfig};
use crate::core::files::Files;

/// Local runner - spawns workers as local processes
pub struct LocalRunner {
    // No configuration needed for local runner
}

impl LocalRunner {
    /// Create a new local runner
    pub fn new() -> Self {
        Self {}
    }

    /// Check if a process is still alive
    pub fn is_pid_alive(pid: u32) -> bool {
        #[cfg(unix)]
        {
            // Send signal 0 to check if process exists
            unsafe { libc::kill(pid as i32, 0) == 0 }
        }

        #[cfg(not(unix))]
        {
            // On non-Unix platforms, try to read /proc/{pid}
            std::path::Path::new(&format!("/proc/{}", pid)).exists()
        }
    }

    /// Kill a process group
    #[cfg(unix)]
    pub fn kill_process_group(pid: u32, signal: i32) {
        unsafe {
            libc::kill(-(pid as i32), signal);
        }
    }
}

impl Default for LocalRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Runner for LocalRunner {
    async fn spawn(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult> {
        // Create log file path
        let files = Files::new(&config.run_dir);
        let log_file = files.worker_log(&config.worker_name);
        if let Some(parent) = log_file.parent() {
            std::fs::create_dir_all(parent).map_err(RunnerError::Io)?;
        }

        debug!("[{}] Worker log file: {:?}", config.worker_name, log_file);

        // Build environment for worker subprocess
        let mut env: HashMap<String, String> = std::env::vars().collect();
        env.insert(
            "ACP_PERMISSION_MODE".to_string(),
            "bypassPermissions".to_string(),
        );
        env.insert("HIRSEL_WORKER_SUBPROCESS".to_string(), "1".to_string());
        env.insert("HIRSEL_RUN".to_string(), config.run_name.clone());
        env.insert("HIRSEL_WORKER".to_string(), config.worker_name.clone());

        // Set agent command for resume_awaiting_workers in worker subprocess
        if let Ok(agent_cmd_json) = serde_json::to_string(&config.agent_command) {
            env.insert("HIRSEL_AGENT_COMMAND".to_string(), agent_cmd_json);
        }

        // Merge any additional env vars from config
        if let Some(ref extra_env) = config.env_vars {
            for (k, v) in extra_env {
                env.insert(k.clone(), v.clone());
            }
        }

        // Get the current executable path
        let hirsel_exe = std::env::current_exe()
            .map_err(|e| RunnerError::SpawnFailed(format!("Failed to get current exe: {}", e)))?;

        // Build args for hirsel __worker-run
        let agent_command_json = serde_json::to_string(&config.agent_command).map_err(|e| {
            RunnerError::SpawnFailed(format!("Failed to serialize agent command: {}", e))
        })?;

        let mut args = vec![
            "__worker-run".to_string(),
            "--run".to_string(),
            config.run_name.clone(),
            "--worker".to_string(),
            config.worker_name.clone(),
            "--work-dir".to_string(),
            config.work_dir.to_string_lossy().to_string(),
            "--run-dir".to_string(),
            config.run_dir.to_string_lossy().to_string(),
            "--spec".to_string(),
            config.spec_path.to_string_lossy().to_string(),
            "--log-file".to_string(),
            log_file.to_string_lossy().to_string(),
            "--agent-command".to_string(),
            agent_command_json,
        ];

        if config.is_leader {
            args.push("--is-leader".to_string());
        }

        if let Some(ref leader) = config.leader_name {
            args.push("--leader-name".to_string());
            args.push(leader.clone());
        }

        if let Some(ref teammates) = config.teammates {
            if !teammates.is_empty() {
                args.push("--teammates".to_string());
                args.push(teammates.join(","));
            }
        }

        if let Some(ref session_id) = config.resume_session_id {
            args.push("--resume-session-id".to_string());
            args.push(session_id.clone());
        }

        // Spawn the detached subprocess in its own process group
        // This allows us to kill the entire process tree when stopping workers
        let mut cmd = Command::new(&hirsel_exe);
        cmd.args(&args)
            .current_dir(&config.work_dir)
            .envs(&env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Create a new process group with the child's PID as the group leader
            // This ensures all descendant processes (claude-code-acp, claude) are in the same group
            cmd.process_group(0);
        }

        let child = cmd
            .spawn()
            .map_err(|e| RunnerError::SpawnFailed(e.to_string()))?;

        let pid = child.id();

        info!(
            "Spawned local worker {} (hirsel __worker-run, PID {})",
            config.worker_name, pid
        );

        Ok(SpawnResult {
            handle: WorkerHandle {
                worker_name: config.worker_name.clone(),
                runner_id: pid.to_string(),
                log_file: Some(log_file),
                runner_type: "local".to_string(),
            },
            pid: Some(pid),
        })
    }

    async fn stop(&self, handle: &WorkerHandle) -> RunnerResult<()> {
        let pid: u32 = handle
            .runner_id
            .parse()
            .map_err(|_| RunnerError::StopFailed("Invalid PID".to_string()))?;

        if !Self::is_pid_alive(pid) {
            return Ok(());
        }

        #[cfg(unix)]
        {
            // Kill the entire process group using negative PID
            // First try SIGTERM for graceful shutdown
            Self::kill_process_group(pid, libc::SIGTERM);
        }

        // Give a brief moment for graceful shutdown
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Force kill if still alive
        if Self::is_pid_alive(pid) {
            #[cfg(unix)]
            {
                Self::kill_process_group(pid, libc::SIGKILL);
            }
        }

        info!("Stopped local worker {} (PID {})", handle.worker_name, pid);

        Ok(())
    }

    async fn is_alive(&self, handle: &WorkerHandle) -> bool {
        if let Ok(pid) = handle.runner_id.parse::<u32>() {
            Self::is_pid_alive(pid)
        } else {
            false
        }
    }

    fn runner_type(&self) -> &'static str {
        "local"
    }

    async fn get_logs(&self, handle: &WorkerHandle, lines: usize) -> RunnerResult<String> {
        if let Some(ref log_file) = handle.log_file {
            if log_file.exists() {
                let content = tokio::fs::read_to_string(log_file)
                    .await
                    .map_err(RunnerError::Io)?;
                let log_lines: Vec<&str> = content.lines().collect();
                let start = log_lines.len().saturating_sub(lines);
                Ok(log_lines[start..].join("\n"))
            } else {
                Ok(String::new())
            }
        } else {
            Ok(String::new())
        }
    }
}

/// Pause all workers by killing their process groups.
///
/// Workers can be resumed later from their saved session state.
pub async fn pause_all_local_workers(handles: &[WorkerHandle]) -> Vec<String> {
    let mut paused = Vec::new();
    let runner = LocalRunner::new();

    for handle in handles {
        if handle.runner_type != "local" {
            continue;
        }

        if runner.is_alive(handle).await {
            if let Err(e) = runner.stop(handle).await {
                warn!("Failed to pause worker {}: {}", handle.worker_name, e);
            } else {
                paused.push(handle.worker_name.clone());
            }
        }
    }

    paused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_runner_type() {
        let runner = LocalRunner::new();
        assert_eq!(runner.runner_type(), "local");
    }

    #[test]
    fn test_is_pid_alive_invalid() {
        // PID 0 should not be considered alive
        assert!(!LocalRunner::is_pid_alive(0));
    }
}
