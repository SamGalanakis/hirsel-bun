//! Worker spawning and utilities
//!
//! This module handles:
//! - Spawning worker processes as detached subprocesses
//! - Worker lifecycle tracking (heartbeats, status updates)
//! - Time notifications
//! - Reconciliation of stale workers on startup
//!
//! Note: Core lifecycle operations (pause/resume, eval triggering, scaling)
//! have been moved to the `lifecycle` module for centralized management.

use crate::cli::AgentPreset;
use crate::core::state::{SQLiteState, StateError, Status, WorkerStatus, WorkerUpdate};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use thiserror::Error;
use tracing::{info, warn};

// Re-export WorkerSpawnConfig from runner module for backwards compatibility
pub use crate::core::runner::WorkerSpawnConfig;

/// Errors that can occur during worker operations
#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("State error: {0}")]
    State(#[from] StateError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Run is paused")]
    RunPaused,

    #[error("Worker not found: {0}")]
    WorkerNotFound(String),

    #[error("Process spawn failed: {0}")]
    SpawnFailed(String),

    #[error("Invalid agent configuration")]
    InvalidAgentConfig,
}

pub type WorkerResult<T> = Result<T, WorkerError>;

/// Result of spawning a worker
#[derive(Debug)]
pub struct SpawnResult {
    /// Worker name
    pub worker_name: String,
    /// Process ID of spawned worker
    pub pid: u32,
}

/// Spawn a new worker process
///
/// Creates a detached subprocess running the worker runner, which manages
/// the ACP client and task claim/done cycle.
///
/// Note: Worker status is only set to Working AFTER successful spawn and
/// process alive verification. This prevents race conditions where the
/// status shows Working but the process failed to start.
pub fn spawn_worker(config: WorkerSpawnConfig, state: &SQLiteState) -> WorkerResult<SpawnResult> {
    // Check if run is paused before spawning
    if state.status()? == Status::Paused {
        info!(
            "[{}] spawn_worker: run is paused, not spawning",
            config.worker_name
        );
        state.update_worker(
            &config.worker_name,
            WorkerUpdate {
                status: Some(WorkerStatus::Paused),
                ..Default::default()
            },
        )?;
        return Err(WorkerError::RunPaused);
    }

    // Build environment for worker subprocess BEFORE updating state
    let mut env: HashMap<String, String> = std::env::vars().collect();
    env.insert(
        "ACP_PERMISSION_MODE".to_string(),
        "bypassPermissions".to_string(),
    );
    env.insert("HIRSEL_WORKER_SUBPROCESS".to_string(), "1".to_string());
    env.insert("HIRSEL_RUN".to_string(), config.run_name.clone());
    env.insert("HIRSEL_WORKER".to_string(), config.worker_name.clone());

    // Apply forwarded credentials (for remote orchestrator mode)
    if let Some(ref creds) = config.credentials {
        if let Some(ref token) = creds.claude_access_token {
            env.insert("CLAUDE_ACCESS_TOKEN".to_string(), token.clone());
        }
        if let Some(ref key) = creds.anthropic_api_key {
            env.insert("ANTHROPIC_API_KEY".to_string(), key.clone());
        }
    }

    // Set agent command for lifecycle manager in worker subprocess
    if let Ok(agent_cmd_json) = serde_json::to_string(&config.agent_command) {
        env.insert("HIRSEL_AGENT_COMMAND".to_string(), agent_cmd_json);
    }

    // Get the current executable path
    let hirsel_exe = std::env::current_exe().map_err(|e| {
        let err = WorkerError::SpawnFailed(format!("Failed to get current exe: {}", e));
        // Mark worker as error state on failure
        let _ = state.update_worker(
            &config.worker_name,
            WorkerUpdate {
                status: Some(WorkerStatus::Error),
                ..Default::default()
            },
        );
        err
    })?;

    // Build args for hirsel __worker-run
    let agent_command_json = serde_json::to_string(&config.agent_command).map_err(|e| {
        let err = WorkerError::SpawnFailed(format!("Failed to serialize agent command: {}", e));
        // Mark worker as error state on failure
        let _ = state.update_worker(
            &config.worker_name,
            WorkerUpdate {
                status: Some(WorkerStatus::Error),
                ..Default::default()
            },
        );
        err
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
        // This ensures all descendant processes (hirsel __acp-bridge, claude) are in the same group
        cmd.process_group(0);
    }

    let child = cmd.spawn().map_err(|e| {
        let err = WorkerError::SpawnFailed(e.to_string());
        // Mark worker as error state on spawn failure
        let _ = state.update_worker(
            &config.worker_name,
            WorkerUpdate {
                status: Some(WorkerStatus::Error),
                ..Default::default()
            },
        );
        err
    })?;

    let pid = child.id();

    // Verify the process is actually alive after spawn
    // This catches cases where the process exits immediately
    if !is_pid_alive(pid) {
        warn!(
            "[{}] spawn_worker: process {} died immediately after spawn",
            config.worker_name, pid
        );
        state.update_worker(
            &config.worker_name,
            WorkerUpdate {
                status: Some(WorkerStatus::Error),
                ..Default::default()
            },
        )?;
        return Err(WorkerError::SpawnFailed(format!(
            "Process {} exited immediately after spawn",
            pid
        )));
    }

    // SUCCESS: Update worker with status, PID, and runner info
    // Status is only set to Working AFTER successful spawn and alive check
    state.update_worker(
        &config.worker_name,
        WorkerUpdate {
            status: Some(WorkerStatus::Working),
            pid: Some(pid as i64),
            runner_id: Some(pid.to_string()),
            runner_type: Some("local".to_string()),
            ..Default::default()
        },
    )?;

    info!(
        "Spawned worker {} (hirsel __worker-run, PID {}, runner_type: local)",
        config.worker_name, pid
    );

    Ok(SpawnResult {
        worker_name: config.worker_name,
        pid,
    })
}

/// Check if a process is still alive
pub fn is_pid_alive(pid: u32) -> bool {
    // PID 0 is the kernel scheduler, never a valid user process
    // Also, kill(0, sig) sends to the process group, not PID 0
    if pid == 0 {
        return false;
    }

    #[cfg(unix)]
    {
        // Send signal 0 to check if process exists
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    #[cfg(not(unix))]
    {
        // On non-Unix platforms, try to read /proc/{pid}
        Path::new(&format!("/proc/{}", pid)).exists()
    }
}

/// Check worker heartbeats and mark stale workers
pub fn check_worker_heartbeats(
    state: &SQLiteState,
    timeout_seconds: i64,
) -> WorkerResult<Vec<String>> {
    let workers = state.get_workers()?;
    let now = chrono::Utc::now();
    let mut stale = Vec::new();

    for worker in workers {
        // Only check workers that should be running (Working or Awaiting with hitl_waiting)
        if worker.status != WorkerStatus::Working
            && !(worker.status == WorkerStatus::Awaiting && worker.hitl_waiting)
        {
            continue;
        }

        // Check if process is still alive
        if let Some(pid) = worker.pid {
            if !is_pid_alive(pid as u32) {
                // Process died - mark as error
                state.update_worker(
                    &worker.name,
                    WorkerUpdate {
                        pid: None,
                        status: Some(WorkerStatus::Error),
                        ..Default::default()
                    },
                )?;
                stale.push(worker.name.clone());
                warn!("Worker {} process died (PID {})", worker.name, pid);
                continue;
            }
        }

        // Check heartbeat timestamp
        if let Some(ref heartbeat) = worker.last_heartbeat {
            if let Ok(heartbeat_time) = chrono::DateTime::parse_from_rfc3339(heartbeat) {
                let elapsed = now.signed_duration_since(heartbeat_time.with_timezone(&chrono::Utc));
                if elapsed.num_seconds() > timeout_seconds {
                    warn!(
                        "Worker {} heartbeat stale ({}s ago)",
                        worker.name,
                        elapsed.num_seconds()
                    );
                    stale.push(worker.name.clone());
                }
            }
        }
    }

    Ok(stale)
}

/// Update a worker's heartbeat timestamp
pub fn update_worker_heartbeat(state: &SQLiteState, worker_name: &str) -> WorkerResult<()> {
    let timestamp = chrono::Utc::now().to_rfc3339();
    state.update_worker(
        worker_name,
        WorkerUpdate {
            last_heartbeat: Some(timestamp),
            ..Default::default()
        },
    )?;
    Ok(())
}

/// Get the agent command for a preset
pub fn get_agent_command(preset: &AgentPreset) -> Vec<String> {
    preset.command.clone()
}

// =============================================================================
// Time Limit Notifications and Timeout Handling
// =============================================================================

/// Time notification thresholds (accelerating frequency)
const TIME_NOTIFICATION_THRESHOLDS: &[i64] = &[25, 50, 75, 85, 90, 95, 98];

/// Get message for a time notification threshold
fn get_time_notification_message(threshold: i64) -> &'static str {
    match threshold {
        25 => "Time check: 25% elapsed, 75% remaining",
        50 => "Halfway point: 50% of time used",
        75 => "75% of time used. Start wrapping up non-essential tasks.",
        85 => "85% elapsed. Prioritize completing current work.",
        90 => "90% of time elapsed! Focus on essential tasks only.",
        95 => "5% time remaining! Finalize immediately.",
        98 => "2% remaining - run will auto-complete very soon.",
        _ => "Time notification",
    }
}

/// Check time limit and send notifications at threshold crossings.
/// Returns the threshold that was notified, if any.
pub fn check_and_send_time_notifications(
    state: &SQLiteState,
    is_multi_worker: bool,
    worker_name: Option<&str>,
) -> WorkerResult<Option<i64>> {
    let time_info = match state.get_time_info()? {
        Some(info) => info,
        None => return Ok(None),
    };

    let pct_elapsed = time_info.percent_elapsed as i64;
    let last_notified = state.get_last_time_notification_pct()?.unwrap_or(0);

    // Find thresholds we've crossed since last notification
    for &threshold in TIME_NOTIFICATION_THRESHOLDS {
        if threshold > last_notified && pct_elapsed >= threshold {
            let message = get_time_notification_message(threshold);

            // Send to group chat for multi-worker, or worker direct for single
            let thread = if is_multi_worker {
                "group".to_string()
            } else {
                worker_name.unwrap_or("user").to_string()
            };

            // Add message to state
            state.add_message(&thread, "System", message, false)?;

            info!("Time notification sent: {}% - {}", threshold, message);
            state.set_last_time_notification_pct(threshold)?;

            return Ok(Some(threshold));
        }
    }

    Ok(None)
}

// =============================================================================
// Dynamic Worker Scaling
// =============================================================================

/// Configuration for worker scaling
/// Workers is just a max count - always starts with 1 and autoscales up.
#[derive(Debug, Clone)]
pub struct WorkerScale {
    pub max: usize,
}

impl WorkerScale {
    /// Parse a scale string - just an integer for max workers.
    pub fn parse(s: &str) -> Option<Self> {
        // Just a number = max workers
        let max = s.trim().parse().ok()?;
        if max < 1 {
            return None;
        }
        Some(Self { max })
    }

    /// Check if we can scale up from current count
    pub fn can_scale_up(&self, current: usize) -> bool {
        current < self.max
    }
}

// =============================================================================
// Worker Reconciliation (Startup)
// =============================================================================

/// Reconcile worker state on app startup.
///
/// When the app restarts (or crashes), workers may have been killed but their
/// database entries still show them as "Working" or "Waiting". This function
/// scans all runs and marks workers with dead PIDs as Paused.
///
/// Workers marked as Paused can be resumed with their full context using
/// the resume functionality (session_id is preserved).
pub fn reconcile_stale_workers() -> Vec<(String, String)> {
    let mut marked: Vec<(String, String)> = Vec::new();

    // Get the runs directory
    let runs_dir = match dirs::home_dir() {
        Some(home) => home.join(".hirsel").join("runs"),
        None => {
            warn!("[reconcile] Could not determine home directory");
            return marked;
        }
    };

    // Iterate over all run directories
    let entries = match std::fs::read_dir(&runs_dir) {
        Ok(e) => e,
        Err(_) => return marked,
    };

    for entry in entries.flatten() {
        let run_name = entry.file_name().to_string_lossy().to_string();
        let db_path = entry.path().join("hirsel.db");

        if !db_path.exists() {
            continue;
        }

        let state = match SQLiteState::new(db_path) {
            Ok(s) => s,
            Err(e) => {
                warn!(
                    "[reconcile] Failed to open database for {}: {}",
                    run_name, e
                );
                continue;
            }
        };

        let workers = match state.get_workers() {
            Ok(w) => w,
            Err(e) => {
                warn!("[reconcile] Failed to get workers for {}: {}", run_name, e);
                continue;
            }
        };

        for worker in workers {
            // Only check workers that should be running (Working or Awaiting with hitl_waiting)
            let is_active = worker.status == WorkerStatus::Working
                || (worker.status == WorkerStatus::Awaiting && worker.hitl_waiting);
            if !is_active {
                continue;
            }

            // If worker has a PID, check if it's still alive
            if let Some(pid) = worker.pid {
                if !is_pid_alive(pid as u32) {
                    // Process is dead but status shows it should be running
                    // Mark as Paused so it can be resumed
                    info!(
                        "[reconcile] Marking stale worker {} in run {} as Paused (PID {} dead)",
                        worker.name, run_name, pid
                    );

                    if let Err(e) = state.update_worker(
                        &worker.name,
                        WorkerUpdate {
                            pid: None,
                            status: Some(WorkerStatus::Paused),
                            hitl_waiting: Some(false),
                            ..Default::default()
                        },
                    ) {
                        warn!(
                            "[reconcile] Failed to mark worker {} as paused: {}",
                            worker.name, e
                        );
                    } else {
                        marked.push((run_name.clone(), worker.name.clone()));
                    }
                }
            } else if worker.status == WorkerStatus::Working {
                // Worker marked as working but has no PID
                // Skip Docker workers - they use container ID (runner_id) instead of PID
                let is_docker = worker
                    .runner_type
                    .as_ref()
                    .map(|t| t == "docker")
                    .unwrap_or(false);

                if is_docker {
                    // Docker workers are managed by container runtime, not by PID
                    // TODO: Could check if container is still running via docker ps
                    continue;
                }

                // Local worker without PID - stale entry, mark as Paused
                info!(
                    "[reconcile] Marking stale worker {} in run {} as Paused (no PID)",
                    worker.name, run_name
                );

                if let Err(e) = state.update_worker(
                    &worker.name,
                    WorkerUpdate {
                        status: Some(WorkerStatus::Paused),
                        ..Default::default()
                    },
                ) {
                    warn!(
                        "[reconcile] Failed to mark worker {} as paused: {}",
                        worker.name, e
                    );
                } else {
                    marked.push((run_name.clone(), worker.name.clone()));
                }
            }
        }
    }

    if !marked.is_empty() {
        info!(
            "[reconcile] Marked {} stale worker(s) as Paused: {:?}",
            marked.len(),
            marked
        );
    }

    marked
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_get_agent_command() {
        let claude = AgentPreset {
            command: vec!["claude".to_string()],
            description: "Claude Code (native)",
            install_hint: Some("See https://docs.anthropic.com/en/docs/claude-code"),
        };
        assert_eq!(get_agent_command(&claude), vec!["claude"]);

        let opencode = AgentPreset {
            command: vec!["opencode".to_string(), "acp".to_string()],
            description: "OpenCode",
            install_hint: None,
        };
        assert_eq!(get_agent_command(&opencode), vec!["opencode", "acp"]);
    }

    #[test]
    fn test_worker_spawn_config() {
        let config = WorkerSpawnConfig {
            run_name: "my-run".to_string(),
            worker_name: "worker1".to_string(),
            work_dir: PathBuf::from("/tmp/work"),
            run_dir: PathBuf::from("/tmp/run"),
            spec_path: PathBuf::from("/tmp/run/spec.md"),
            agent_command: vec!["test-agent".to_string()],
            is_leader: true,
            leader_name: Some("worker1".to_string()),
            teammates: Some(vec!["worker2".to_string()]),
            resume_session_id: None,
            env_vars: None,
            credentials: None,
            coordinator_url: None,
            tailscale_authkey: None,
        };

        assert!(config.is_leader);
        assert_eq!(config.leader_name, Some("worker1".to_string()));
        assert_eq!(config.teammates, Some(vec!["worker2".to_string()]));
    }
}
