//! Worker spawning and lifecycle management
//!
//! This module handles:
//! - Spawning worker processes as detached subprocesses
//! - Worker lifecycle tracking (heartbeats, status updates)
//! - Pausing and resuming workers
//! - Resuming workers that were awaiting tasks

use crate::cli::AgentPreset;
use crate::core::files::Files;
use crate::core::state::{SQLiteState, StateError, Status, WorkerStatus, WorkerUpdate};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use thiserror::Error;
use tracing::{debug, info, warn};

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

/// Configuration for spawning a worker
#[derive(Debug, Clone)]
pub struct WorkerSpawnConfig {
    /// Run name
    pub run_name: String,
    /// Worker name
    pub worker_name: String,
    /// Working directory for the worker (git worktree)
    pub work_dir: PathBuf,
    /// Run directory (contains state.db, chats/, logs/)
    pub run_dir: PathBuf,
    /// Path to spec file
    pub spec_path: PathBuf,
    /// Agent command to run (e.g., ["claude-code-acp"])
    pub agent_command: Vec<String>,
    /// Whether this worker is the leader
    pub is_leader: bool,
    /// Name of the leader worker (if known)
    pub leader_name: Option<String>,
    /// List of teammate worker names
    pub teammates: Option<Vec<String>>,
    /// Session ID to resume (optional)
    pub resume_session_id: Option<String>,
}

/// Result of spawning a worker
#[derive(Debug)]
pub struct SpawnResult {
    /// Worker name
    pub worker_name: String,
    /// Process ID of spawned worker
    pub pid: u32,
    /// Path to worker's log file
    pub log_file: PathBuf,
}

/// Spawn a new worker process
///
/// Creates a detached subprocess running the worker runner, which manages
/// the ACP client and task claim/done cycle.
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

    // Update worker status to working
    state.update_worker(
        &config.worker_name,
        WorkerUpdate {
            status: Some(WorkerStatus::Working),
            ..Default::default()
        },
    )?;

    // Create log file path
    let files = Files::new(&config.run_dir);
    let log_file = files.worker_log(&config.worker_name);
    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Write initial log entry
    std::fs::write(
        &log_file,
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

    debug!(
        "[{}] Worker log file: {:?}",
        config.worker_name, log_file
    );

    // Build environment for worker subprocess
    let mut env: HashMap<String, String> = std::env::vars().collect();
    env.insert("ACP_PERMISSION_MODE".to_string(), "bypassPermissions".to_string());
    env.insert("HIRSEL_WORKER_SUBPROCESS".to_string(), "1".to_string());
    env.insert("HIRSEL_RUN".to_string(), config.run_name.clone());
    env.insert("HIRSEL_WORKER".to_string(), config.worker_name.clone());

    // Build the worker command
    // The worker subprocess will use hirsel-worker CLI
    let worker_args = build_worker_args(&config);

    // Spawn the detached subprocess
    let child = Command::new(&config.agent_command[0])
        .args(&config.agent_command[1..])
        .args(&worker_args)
        .current_dir(&config.work_dir)
        .envs(&env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| WorkerError::SpawnFailed(e.to_string()))?;

    let pid = child.id();

    // Update worker with PID
    state.update_worker(
        &config.worker_name,
        WorkerUpdate {
            pid: Some(pid as i64),
            ..Default::default()
        },
    )?;

    info!(
        "Spawned worker {} (ACP: {}, PID {})",
        config.worker_name, config.agent_command[0], pid
    );

    Ok(SpawnResult {
        worker_name: config.worker_name,
        pid,
        log_file,
    })
}

/// Build command line arguments for the worker subprocess
fn build_worker_args(config: &WorkerSpawnConfig) -> Vec<String> {
    let mut args = vec![
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
    ];

    if config.is_leader {
        args.push("--leader".to_string());
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
        args.push("--resume".to_string());
        args.push(session_id.clone());
    }

    args
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
        Path::new(&format!("/proc/{}", pid)).exists()
    }
}

/// Pause all workers in a run by sending SIGTERM
pub fn pause_all_workers(state: &SQLiteState) -> WorkerResult<Vec<String>> {
    let workers = state.get_workers()?;
    let mut paused = Vec::new();

    for worker in workers {
        if let Some(pid) = worker.pid {
            if is_pid_alive(pid as u32) {
                #[cfg(unix)]
                {
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
                    }
                }
                info!("Paused worker {} (PID {})", worker.name, pid);
            }
        }

        // Mark as paused regardless of whether process was running
        if worker.status != WorkerStatus::Done && worker.status != WorkerStatus::Error {
            state.update_worker(
                &worker.name,
                WorkerUpdate {
                    pid: None,
                    status: Some(WorkerStatus::Paused),
                    ..Default::default()
                },
            )?;
            paused.push(worker.name.clone());
        }
    }

    Ok(paused)
}

/// Resume workers that are in the awaiting state (waiting for tasks)
pub fn resume_awaiting_workers(
    run_name: &str,
    run_dir: &Path,
    agent_command: &[String],
) -> WorkerResult<Vec<String>> {
    let files = Files::new(run_dir);
    let state = SQLiteState::new(files.db_path())?;

    // Get claimable tasks
    let claimable = state.get_claimable_tasks()?;
    if claimable.is_empty() {
        return Ok(Vec::new());
    }

    // Get awaiting workers
    let workers = state.get_workers()?;
    let awaiting: Vec<_> = workers
        .iter()
        .filter(|w| w.status == WorkerStatus::Awaiting)
        .collect();

    if awaiting.is_empty() {
        return Ok(Vec::new());
    }

    let mut resumed = Vec::new();

    for worker in awaiting {
        let work_dir = worker
            .work_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| run_dir.join("workers").join(&worker.name));

        let config = WorkerSpawnConfig {
            run_name: run_name.to_string(),
            worker_name: worker.name.clone(),
            work_dir,
            run_dir: run_dir.to_path_buf(),
            spec_path: files.spec(),
            agent_command: agent_command.to_vec(),
            is_leader: false,
            leader_name: None,
            teammates: None,
            resume_session_id: worker.session_id.clone(),
        };

        match spawn_worker(config, &state) {
            Ok(_) => {
                resumed.push(worker.name.clone());
            }
            Err(WorkerError::RunPaused) => {
                // Run was paused while we were processing
                break;
            }
            Err(e) => {
                warn!("Failed to resume worker {}: {}", worker.name, e);
            }
        }
    }

    Ok(resumed)
}

/// Check worker heartbeats and mark stale workers
pub fn check_worker_heartbeats(state: &SQLiteState, timeout_seconds: i64) -> WorkerResult<Vec<String>> {
    let workers = state.get_workers()?;
    let now = chrono::Utc::now();
    let mut stale = Vec::new();

    for worker in workers {
        // Only check workers that should be running
        if worker.status != WorkerStatus::Working && worker.status != WorkerStatus::Waiting {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_worker_args_basic() {
        let config = WorkerSpawnConfig {
            run_name: "test-run".to_string(),
            worker_name: "alpha".to_string(),
            work_dir: PathBuf::from("/work/alpha"),
            run_dir: PathBuf::from("/runs/test-run"),
            spec_path: PathBuf::from("/runs/test-run/spec.md"),
            agent_command: vec!["claude-code-acp".to_string()],
            is_leader: false,
            leader_name: None,
            teammates: None,
            resume_session_id: None,
        };

        let args = build_worker_args(&config);
        assert!(args.contains(&"--run".to_string()));
        assert!(args.contains(&"test-run".to_string()));
        assert!(args.contains(&"--worker".to_string()));
        assert!(args.contains(&"alpha".to_string()));
        assert!(!args.contains(&"--leader".to_string()));
    }

    #[test]
    fn test_build_worker_args_leader() {
        let config = WorkerSpawnConfig {
            run_name: "test-run".to_string(),
            worker_name: "alpha".to_string(),
            work_dir: PathBuf::from("/work/alpha"),
            run_dir: PathBuf::from("/runs/test-run"),
            spec_path: PathBuf::from("/runs/test-run/spec.md"),
            agent_command: vec!["claude-code-acp".to_string()],
            is_leader: true,
            leader_name: Some("alpha".to_string()),
            teammates: Some(vec!["beta".to_string(), "gamma".to_string()]),
            resume_session_id: None,
        };

        let args = build_worker_args(&config);
        assert!(args.contains(&"--leader".to_string()));
        assert!(args.contains(&"--leader-name".to_string()));
        assert!(args.contains(&"--teammates".to_string()));
        assert!(args.contains(&"beta,gamma".to_string()));
    }

    #[test]
    fn test_build_worker_args_resume() {
        let config = WorkerSpawnConfig {
            run_name: "test-run".to_string(),
            worker_name: "alpha".to_string(),
            work_dir: PathBuf::from("/work/alpha"),
            run_dir: PathBuf::from("/runs/test-run"),
            spec_path: PathBuf::from("/runs/test-run/spec.md"),
            agent_command: vec!["claude-code-acp".to_string()],
            is_leader: false,
            leader_name: None,
            teammates: None,
            resume_session_id: Some("session-123".to_string()),
        };

        let args = build_worker_args(&config);
        assert!(args.contains(&"--resume".to_string()));
        assert!(args.contains(&"session-123".to_string()));
    }

    #[test]
    fn test_get_agent_command() {
        let claude = AgentPreset {
            command: vec!["claude-code-acp".to_string()],
            description: "Claude Code",
            install_hint: Some("npm install -g @anthropics/claude-code-acp"),
        };
        assert_eq!(get_agent_command(&claude), vec!["claude-code-acp"]);

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
        };

        assert!(config.is_leader);
        assert_eq!(config.leader_name, Some("worker1".to_string()));
        assert_eq!(config.teammates, Some(vec!["worker2".to_string()]));
    }
}
