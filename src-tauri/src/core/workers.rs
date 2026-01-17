//! Worker spawning and lifecycle management
//!
//! This module handles:
//! - Spawning worker processes as detached subprocesses
//! - Worker lifecycle tracking (heartbeats, status updates)
//! - Pausing and resuming workers
//! - Resuming workers that were awaiting tasks

use crate::cli::AgentPreset;
use crate::core::config::Config;
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

    // Get the current executable path
    let hirsel_exe = std::env::current_exe()
        .map_err(|e| WorkerError::SpawnFailed(format!("Failed to get current exe: {}", e)))?;

    // Build args for hirsel __worker-run
    let agent_command_json = serde_json::to_string(&config.agent_command).map_err(|e| {
        WorkerError::SpawnFailed(format!("Failed to serialize agent command: {}", e))
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
        "Spawned worker {} (hirsel __worker-run, PID {})",
        config.worker_name, pid
    );

    Ok(SpawnResult {
        worker_name: config.worker_name,
        pid,
        log_file,
    })
}

/// Build command line arguments for the worker subprocess
#[allow(dead_code)]
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

/// Pause all workers in a run by killing their process groups.
///
/// Workers can be resumed later from their saved session state.
/// Since workers are spawned with process_group(0), killing the process group
/// will also kill all child processes (claude-code-acp, claude, etc.).
pub fn pause_all_workers(state: &SQLiteState) -> WorkerResult<Vec<String>> {
    let workers = state.get_workers()?;
    let mut paused = Vec::new();

    for worker in workers {
        if let Some(pid) = worker.pid {
            // Always attempt to kill the process group, even if leader appears dead
            // The leader (hirsel subprocess) dies quickly on SIGTERM, but children
            // (claude-code-acp, claude) may survive and need SIGKILL
            #[cfg(unix)]
            {
                // Kill the entire process group using negative PID
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGTERM);
                }
            }
            // Brief wait for graceful shutdown
            std::thread::sleep(std::time::Duration::from_millis(100));

            // ALWAYS send SIGKILL to process group - children may survive even if leader died
            // (Node.js processes like claude-code-acp may ignore SIGTERM)
            #[cfg(unix)]
            {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            info!("Killed worker {} process group (PID {})", worker.name, pid);
        }

        // Mark as paused regardless of whether process was running (skip already inactive workers)
        if !worker.status.is_inactive() {
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

/// Kill all workers in a run by sending signals to the entire process group.
///
/// This is a forceful cleanup used when a run reaches a terminal state
/// (Done, EvalFailed, TimedOut) to ensure no orphaned worker processes remain.
/// Since workers are spawned with process_group(0), killing the process group
/// will also kill all child processes (claude-code-acp, claude, etc.).
pub fn kill_all_workers(state: &SQLiteState) -> WorkerResult<Vec<String>> {
    let workers = state.get_workers()?;
    let mut killed = Vec::new();

    for worker in workers {
        if let Some(pid) = worker.pid {
            // Always attempt to kill the process group, even if leader appears dead
            // The leader (hirsel subprocess) dies quickly on SIGTERM, but children
            // (claude-code-acp, claude) may survive and need SIGKILL
            #[cfg(unix)]
            {
                // Kill the entire process group using negative PID
                // This kills the worker and all its children (claude-code-acp, claude)
                unsafe {
                    // First try SIGTERM for graceful shutdown of the process group
                    libc::kill(-(pid as i32), libc::SIGTERM);
                }
            }
            // Give a brief moment for graceful shutdown
            std::thread::sleep(std::time::Duration::from_millis(100));

            // ALWAYS send SIGKILL to process group - children may survive even if leader died
            // (Node.js processes like claude-code-acp may ignore SIGTERM)
            #[cfg(unix)]
            {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            info!("Killed worker {} process group (PID {})", worker.name, pid);
            killed.push(worker.name.clone());
        }

        // Clear PID from database
        if worker.pid.is_some() {
            let _ = state.update_worker(
                &worker.name,
                WorkerUpdate {
                    pid: None,
                    ..Default::default()
                },
            );
        }
    }

    Ok(killed)
}

/// Resume workers that are in the awaiting state (waiting for tasks)
pub fn resume_awaiting_workers(
    run_name: &str,
    run_dir: &Path,
    agent_command: &[String],
) -> WorkerResult<Vec<String>> {
    let files = Files::new(run_dir);
    let state = SQLiteState::new(files.db_path())?;

    // Get claimable tasks (needed for awaiting workers)
    let claimable = state.get_claimable_tasks()?;

    // Get workers that need to be resumed:
    // - Paused: were actively working when run was paused, resume unconditionally
    // - Error: worker died unexpectedly (e.g., app restart), resume unconditionally
    // - Awaiting: waiting for tasks, only resume if tasks available
    let workers = state.get_workers()?;
    let to_resume: Vec<_> = workers
        .iter()
        .filter(|w| {
            w.status == WorkerStatus::Paused
                || w.status == WorkerStatus::Error
                || (w.status == WorkerStatus::Awaiting && !claimable.is_empty())
        })
        .collect();

    if to_resume.is_empty() {
        return Ok(Vec::new());
    }

    let mut resumed = Vec::new();

    for worker in to_resume {
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
pub fn check_worker_heartbeats(
    state: &SQLiteState,
    timeout_seconds: i64,
) -> WorkerResult<Vec<String>> {
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

/// Handle time limit expiration - set run to TIMED_OUT status.
/// Should only be called by the first worker to detect expiration.
pub fn handle_time_expired(
    state: &SQLiteState,
    files: &Files,
    is_multi_worker: bool,
    worker_name: &str,
    run_name: &str,
) -> WorkerResult<()> {
    use crate::core::state::Status;

    // Only the first worker to detect expiration should handle it
    if state.status()? == Status::TimedOut {
        info!("[{}] Time already expired, skipping handler", worker_name);
        return Ok(());
    }

    // Send final message
    let message = "Time limit reached. Run paused with current progress.";
    let thread = if is_multi_worker {
        "group"
    } else {
        worker_name
    };

    state.add_message(thread, "System", message, false)?;

    // Cancel any running evals
    let cancelled = state.cancel_running_evals("Time limit reached")?;
    if cancelled > 0 {
        info!(
            "[{}] Cancelled {} running eval(s) due to timeout",
            worker_name, cancelled
        );
    }

    // Kill all worker processes and mark them as paused
    let killed = kill_all_workers(state)?;
    if !killed.is_empty() {
        info!(
            "[{}] Killed {} worker(s) on timeout: {:?}",
            worker_name,
            killed.len(),
            killed
        );
    }

    // Set all active workers to PAUSED status
    let workers = state.get_workers()?;
    for worker in workers {
        if matches!(
            worker.status,
            WorkerStatus::Working | WorkerStatus::Waiting | WorkerStatus::Awaiting
        ) {
            state.update_worker(
                &worker.name,
                WorkerUpdate {
                    status: Some(WorkerStatus::Paused),
                    ..Default::default()
                },
            )?;
        }
    }

    // Set run status to TIMED_OUT
    state.set_status(Status::TimedOut)?;
    info!("[{}] Run status set to TIMED_OUT", worker_name);

    // Write to worker log
    let log_file = files.worker_log(worker_name);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
    {
        use std::io::Write;
        let _ = writeln!(file, "\n[time limit reached - run timed out]");
    }

    // Trigger summary generation in background
    spawn_background_summary(run_name, worker_name);

    Ok(())
}

/// Spawn summary generation in a background process
fn spawn_background_summary(run_name: &str, worker_name: &str) {
    let hirsel_exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            warn!(
                "[{}] Failed to get current exe for summary: {}",
                worker_name, e
            );
            return;
        }
    };

    match Command::new(&hirsel_exe)
        .args(["summary", run_name])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => {
            info!(
                "[{}] Spawned summary generation for timed out run",
                worker_name
            );
        }
        Err(e) => {
            warn!(
                "[{}] Failed to spawn summary generation: {}",
                worker_name, e
            );
        }
    }
}

/// Check if time has expired and handle it if so.
/// Returns true if time expired and was handled.
pub fn check_time_expired(
    state: &SQLiteState,
    files: &Files,
    is_multi_worker: bool,
    worker_name: &str,
    run_name: &str,
) -> WorkerResult<bool> {
    if state.is_time_expired()? {
        handle_time_expired(state, files, is_multi_worker, worker_name, run_name)?;
        Ok(true)
    } else {
        Ok(false)
    }
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

/// Check if we should scale up workers based on autoscale settings.
/// Returns the name of the new worker if one was spawned, None otherwise.
pub fn maybe_scale_up(
    run_name: &str,
    run_dir: &Path,
    agent_command: &[String],
) -> WorkerResult<Option<String>> {
    use crate::core::git::create_worker_clone;

    let files = Files::new(run_dir);
    let state = SQLiteState::new(files.db_path())?;

    // Check if autoscaling is enabled
    let scale_str = match state.get_worker_scale()? {
        Some(s) => s,
        None => return Ok(None),
    };

    let scale = match WorkerScale::parse(&scale_str) {
        Some(s) => s,
        None => return Ok(None),
    };

    // Don't scale up if run is paused
    use crate::core::state::Status;
    if state.status()? == Status::Paused {
        debug!("maybe_scale_up: run is paused, not scaling");
        return Ok(None);
    }

    // Get current workers and claimable tasks
    let workers = state.get_workers()?;
    let current_count = workers.len();
    let claimable = state.get_claimable_tasks()?;
    let claimable_count = claimable.len();

    // Target is min(max_workers, claimable_tasks)
    let target_workers = std::cmp::min(scale.max, claimable_count);

    debug!(
        "maybe_scale_up: {} claimable tasks, {} workers, max {}, target {}",
        claimable_count, current_count, scale.max, target_workers
    );

    // Only scale up if we have fewer workers than target
    if current_count >= target_workers {
        return Ok(None);
    }

    // Get a new worker name
    let existing_names: Vec<String> = workers.iter().map(|w| w.name.clone()).collect();
    let new_name = crate::cli::go::get_available_name(&existing_names);

    // Get project path
    let project_path_str = match state.get_project_path()? {
        Some(p) => p,
        None => {
            warn!("maybe_scale_up: no project path, cannot scale");
            return Ok(None);
        }
    };

    let project_path = PathBuf::from(&project_path_str);
    let staging_dir = run_dir.join("work").join("staging");

    // Create worker clone
    let worker_dir = match create_worker_clone(
        run_name,
        &project_path,
        &new_name,
        Some(&staging_dir),
        run_dir,
    ) {
        Ok(dir) => dir,
        Err(e) => {
            warn!("maybe_scale_up: failed to create worker clone: {}", e);
            return Ok(None);
        }
    };

    // Add worker to state
    state.add_worker(&new_name, worker_dir.to_str().unwrap_or("."), "local")?;

    // Create worker chat file
    let chat_file = files.chats_dir().join(format!("{}.md", new_name));
    if let Err(e) = std::fs::write(&chat_file, format!("# {} Chat\n\n", new_name)) {
        warn!("maybe_scale_up: failed to create worker chat: {}", e);
    }

    // Announce in group chat
    let reason = format!(
        "Autoscaling: {} tasks available, {} workers total",
        claimable.len(),
        current_count
    );
    state.add_message(
        "group",
        "System",
        &format!(
            "New worker **{}** has joined the team. {}",
            new_name, reason
        ),
        false,
    )?;

    // Get leader info
    let leader = workers.iter().find(|w| {
        // First worker is typically the leader
        workers.iter().position(|x| x.name == w.name) == Some(0)
    });
    let leader_name = leader.map(|l| l.name.clone());

    // Get teammates
    let teammates: Vec<String> = workers.iter().map(|w| w.name.clone()).collect();

    // Spawn the worker
    let config = WorkerSpawnConfig {
        run_name: run_name.to_string(),
        worker_name: new_name.clone(),
        work_dir: worker_dir,
        run_dir: run_dir.to_path_buf(),
        spec_path: files.spec(),
        agent_command: agent_command.to_vec(),
        is_leader: false,
        leader_name,
        teammates: Some(teammates),
        resume_session_id: None,
    };

    match spawn_worker(config, &state) {
        Ok(result) => {
            info!(
                "Scaled up: spawned new worker {} (PID {})",
                new_name, result.pid
            );
            Ok(Some(new_name))
        }
        Err(e) => {
            warn!("maybe_scale_up: failed to spawn worker: {}", e);
            Ok(None)
        }
    }
}

// =============================================================================
// Eval Triggering
// =============================================================================

/// Check if all workers are inactive and maybe trigger eval.
/// This should be called whenever a worker transitions to Awaiting or Error status.
/// Returns true if eval was triggered.
pub fn maybe_trigger_eval(_run_name: &str, run_dir: &Path) -> WorkerResult<bool> {
    let files = Files::new(run_dir);
    let state = SQLiteState::new(files.db_path())?;

    // Check if all workers are inactive
    if !state.all_workers_inactive()? {
        debug!("maybe_trigger_eval: not all workers inactive, skipping");
        return Ok(false);
    }

    // Check if run is still in working status
    let status = state.status()?;
    if status != Status::Working {
        debug!(
            "maybe_trigger_eval: run status is {:?}, not Working, skipping",
            status
        );
        return Ok(false);
    }

    // Check if there's an eval script configured
    let eval_path = files.eval_spec();
    if !eval_path.exists() {
        // No eval script - kill any remaining workers and set run to Done status
        let killed = kill_all_workers(&state)?;
        if !killed.is_empty() {
            info!(
                "maybe_trigger_eval: killed {} remaining worker(s): {:?}",
                killed.len(),
                killed
            );
        }

        info!("maybe_trigger_eval: all workers inactive, no eval script, marking run as Done");
        state.set_status(Status::Done)?;

        // Trigger auto-improve if enabled
        let (config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));
        let _ = maybe_run_improve(_run_name, &config);

        return Ok(false);
    }

    // Trigger eval
    info!("maybe_trigger_eval: all workers inactive, triggering eval");
    state.set_status(Status::Eval)?;

    // Spawn the eval agent in a background process
    spawn_eval_agent(run_dir, &files)?;

    Ok(true)
}

/// Spawn the eval agent as a background process.
/// This runs the eval asynchronously and updates the run status when complete.
fn spawn_eval_agent(run_dir: &Path, files: &Files) -> WorkerResult<()> {
    use std::process::{Command, Stdio};

    // Get the hirsel executable
    let hirsel_exe =
        std::env::current_exe().map_err(|e| WorkerError::Io(std::io::Error::other(e)))?;

    // Get run name from run_dir
    let run_name = run_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Get agent command from config
    let (config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));
    let agent_command = config.agent.command.clone();

    // Spawn the eval subprocess
    // The __eval-run command will handle the actual eval execution
    let mut cmd = Command::new(&hirsel_exe);
    cmd.arg("__eval-run")
        .arg("--run")
        .arg(run_name)
        .arg("--run-dir")
        .arg(run_dir)
        .arg("--spec")
        .arg(files.spec())
        .arg("--eval-spec")
        .arg(files.eval_spec())
        .arg("--agent-command")
        .arg(serde_json::to_string(&agent_command).unwrap_or_else(|_| "[]".to_string()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Spawn detached
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let child = cmd
        .spawn()
        .map_err(|e| WorkerError::SpawnFailed(format!("Failed to spawn eval agent: {}", e)))?;

    info!(
        "maybe_trigger_eval: spawned eval agent for run {}, pid={}",
        run_name,
        child.id()
    );

    Ok(())
}

/// Internal cooldown for compaction checks (10 seconds)
const COMPACTION_INTERNAL_COOLDOWN_SECONDS: i64 = 10;

/// Check if learnings compaction should run, and if so, trigger it.
///
/// This function checks:
/// 1. If compaction is enabled in config
/// 2. If the internal cooldown (10s) has elapsed since the last compaction
/// 3. If there are enough messages to warrant compaction
///
/// If all conditions are met, it spawns an async task to perform compaction.
/// Returns true if compaction was triggered.
pub async fn maybe_compact_learnings(
    state: &SQLiteState,
    files: &Files,
    config: &Config,
) -> WorkerResult<bool> {
    use chrono::{DateTime, Utc};

    // Quick check: is compaction enabled?
    if !config.compaction_enabled {
        debug!("maybe_compact_learnings: compaction disabled");
        return Ok(false);
    }

    // Internal cooldown check (10 seconds) - just to prevent rapid-fire triggers
    if let Some(last_compaction) = state.get_last_compaction_at()? {
        if let Ok(last_time) = DateTime::parse_from_rfc3339(&last_compaction) {
            let now = Utc::now();
            let elapsed_seconds = (now - last_time.with_timezone(&Utc)).num_seconds();

            if elapsed_seconds < COMPACTION_INTERNAL_COOLDOWN_SECONDS {
                warn!(
                    "maybe_compact_learnings: triggered within {}s of last compaction ({}s ago)",
                    COMPACTION_INTERNAL_COOLDOWN_SECONDS, elapsed_seconds
                );
                return Ok(false);
            }
        }
    }

    // Try to run compaction
    use crate::core::compaction::{compact_learnings_with_agent, CompactionError};

    match compact_learnings_with_agent(state, files, config).await {
        Ok(result) => {
            info!(
                "maybe_compact_learnings: compacted {} messages",
                result.messages_compacted
            );

            // Update the last compaction timestamp
            let now = Utc::now().to_rfc3339();
            if let Err(e) = state.set_last_compaction_at(&now) {
                warn!("Failed to update last_compaction_at: {}", e);
            }

            Ok(true)
        }
        Err(CompactionError::NotNeeded) => {
            debug!("maybe_compact_learnings: compaction not needed");
            Ok(false)
        }
        Err(e) => {
            warn!("maybe_compact_learnings: compaction failed: {}", e);
            Ok(false)
        }
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
            // Only check workers that should be running
            if !matches!(
                worker.status,
                WorkerStatus::Working | WorkerStatus::Waiting | WorkerStatus::Idle
            ) {
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
            } else if matches!(worker.status, WorkerStatus::Working | WorkerStatus::Waiting) {
                // Worker marked as working/waiting but has no PID - stale entry
                // Mark as Paused so it can be resumed
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

// =============================================================================
// Auto-improve
// =============================================================================

/// Run the improve agent to update project memory from learnings.
///
/// This should be called when a run completes (status changes to Done/Delivered).
/// If auto_improve is enabled in config, it spawns the improve agent to analyze
/// learnings and update the project memory file (CLAUDE.md or AGENTS.md).
pub fn maybe_run_improve(run_name: &str, config: &Config) -> WorkerResult<bool> {
    // Check if auto-improve is enabled
    if !config.auto_improve {
        debug!("maybe_run_improve: auto_improve disabled");
        return Ok(false);
    }

    info!(
        "maybe_run_improve: auto-improve enabled, running improve for {}",
        run_name
    );

    // Spawn the improve as a separate process to not block
    // Use the CLI improve command with json output
    let hirsel_exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            warn!("maybe_run_improve: failed to get current exe: {}", e);
            return Ok(false);
        }
    };

    let mut cmd = Command::new(&hirsel_exe);
    cmd.arg("improve")
        .arg("--run")
        .arg(run_name)
        .arg("--json")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Spawn detached
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    match cmd.spawn() {
        Ok(child) => {
            info!(
                "maybe_run_improve: spawned improve agent for {}, pid={}",
                run_name,
                child.id()
            );
            Ok(true)
        }
        Err(e) => {
            warn!("maybe_run_improve: failed to spawn improve agent: {}", e);
            Ok(false)
        }
    }
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
