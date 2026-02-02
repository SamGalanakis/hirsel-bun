//! Worker-related commands
//!
//! Commands for managing workers: listing, attaching, detaching, opening terminal, and restarting.

use crate::core::api_types::{SheepConfig, Worker, WorkerLocation, WorkerStatus};
use crate::core::config;
use crate::core::orchestrator::create_orchestrator;

use super::{err_string, get_run_state};

/// Get all workers for a run
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn get_workers(run_name: String) -> Result<Vec<Worker>, String> {
    let orch = create_orchestrator(None).map_err(err_string)?;
    orch.list_workers(&run_name).await.map_err(err_string)
}

/// Attach a new worker to a run
///
/// Creates a new worker with the given name, sets up its working directory,
/// and spawns the worker process.
#[tauri::command]
pub async fn attach_worker(run_name: String, worker_name: String) -> Result<Worker, String> {
    use crate::cli::config::get_agent_command;
    use crate::core::git::create_worker_clone;
    use crate::core::workers::{spawn_worker, WorkerSpawnConfig};
    use crate::core::Files;

    let run_dir = config::run_dir(&run_name);
    let state = get_run_state(&run_name)?;

    // Check if worker already exists
    if state.get_worker(&worker_name).ok().flatten().is_some() {
        return Err(format!("Worker '{}' already exists", worker_name));
    }

    // Get project path
    let project_path_str = state
        .get_project_path()
        .map_err(|e| format!("Failed to get project path: {}", e))?
        .ok_or_else(|| "No project path configured".to_string())?;
    let project_path = std::path::PathBuf::from(&project_path_str);

    // Get existing workers to determine if multi-worker
    let workers = state
        .get_workers()
        .map_err(|e| format!("Failed to get workers: {}", e))?;
    let is_multi_worker = !workers.is_empty();

    // Create worker clone/worktree
    let staging_dir = run_dir.join("work").join("staging");
    let runs_dir = config::runs_dir();
    let worker_dir = create_worker_clone(
        &run_name,
        &project_path,
        &worker_name,
        Some(&staging_dir),
        &runs_dir,
    )
    .map_err(|e| format!("Failed to create worker clone: {}", e))?;

    // Add worker to state
    state
        .add_worker(&worker_name, worker_dir.to_str().unwrap_or("."), "local")
        .map_err(|e| format!("Failed to add worker: {}", e))?;

    // Create worker chat file
    let files = Files::new(&run_dir);
    let chat_file = files.chats_dir().join(format!("{}.md", worker_name));
    let _ = std::fs::write(&chat_file, format!("# {} Chat\n\n", worker_name));

    // Get leader info
    let leader_name = workers.first().map(|w| w.name.clone());
    let teammates: Vec<String> = workers.iter().map(|w| w.name.clone()).collect();

    // Spawn the worker
    let agent_command = get_agent_command();
    let config = WorkerSpawnConfig {
        run_name: run_name.clone(),
        worker_name: worker_name.clone(),
        work_dir: worker_dir.clone(),
        run_dir: run_dir.clone(),
        agent_command,
        is_leader: false,
        leader_name,
        teammates: if is_multi_worker {
            Some(teammates)
        } else {
            None
        },
        resume_session_id: None,
        env_vars: None,
        credentials: None,
        coordinator_url: None,
        tailscale_authkey: None,
        assigned_task_id: None,
    };

    match spawn_worker(config, &state) {
        Ok(result) => {
            tracing::info!("Attached worker {} (PID {})", worker_name, result.pid);
        }
        Err(e) => {
            return Err(format!("Failed to spawn worker: {}", e));
        }
    }

    // Return the created worker
    let worker = state
        .get_worker(&worker_name)
        .map_err(|e| format!("Failed to get worker: {}", e))?
        .ok_or_else(|| "Worker not found after creation".to_string())?;

    Ok(Worker {
        id: worker.id as u32,
        name: worker.name.clone(),
        pid: worker.pid.map(|p| p as u32),
        session_id: worker.session_id,
        status: match worker.status {
            crate::core::state::WorkerStatus::Working => WorkerStatus::Working,
            crate::core::state::WorkerStatus::Awaiting => WorkerStatus::Awaiting,
            crate::core::state::WorkerStatus::Paused => WorkerStatus::Paused,
            crate::core::state::WorkerStatus::Error => WorkerStatus::Error,
        },
        work_dir: worker.work_dir,
        waiting_thread: worker.waiting_thread,
        location: WorkerLocation::Local,
        last_heartbeat: worker.last_heartbeat,
        created_at: worker.created_at,
        needs_restart: worker.needs_restart,
        session_started_at: worker.session_started_at,
        hitl_waiting: worker.hitl_waiting,
        is_leader: false,
        context_utilization: None,
        input_tokens: None,
        output_tokens: None,
        turns: None,
        current_task: None,
        sheep_config: SheepConfig::from_name(&worker.name, false),
    })
}

/// Open an external terminal attached to a worker's tmux session
///
/// This opens a new terminal window running `tmux attach-session` for the worker.
#[tauri::command]
pub async fn open_worker_terminal(run_name: String, worker_name: String) -> Result<(), String> {
    use std::process::Command;

    let state = get_run_state(&run_name)?;

    // Verify worker exists
    let workers = state
        .get_workers()
        .map_err(|e| format!("Failed to get workers: {}", e))?;

    if !workers.iter().any(|w| w.name == worker_name) {
        return Err(format!("Worker '{}' not found", worker_name));
    }

    // Check if tmux session exists
    let session_name = format!("hirsel-{}-{}", run_name, worker_name);
    let session_exists = Command::new("tmux")
        .args(["has-session", "-t", &session_name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !session_exists {
        return Err(format!(
            "No tmux session '{}' found. Worker '{}' may not be running.",
            session_name, worker_name
        ));
    }

    // Try to open a terminal with tmux attach
    // Try common terminal emulators in order of preference
    let attach_cmd = format!("tmux attach-session -t {}", session_name);

    let terminals = [
        ("alacritty", vec!["-e", "sh", "-c", &attach_cmd]),
        ("kitty", vec!["sh", "-c", &attach_cmd]),
        ("wezterm", vec!["start", "--", "sh", "-c", &attach_cmd]),
        ("gnome-terminal", vec!["--", "sh", "-c", &attach_cmd]),
        ("konsole", vec!["-e", "sh", "-c", &attach_cmd]),
        ("xterm", vec!["-e", "sh", "-c", &attach_cmd]),
        ("x-terminal-emulator", vec!["-e", "sh", "-c", &attach_cmd]),
    ];

    for (term, args) in &terminals {
        if Command::new("which")
            .arg(term)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            match Command::new(term).args(args).spawn() {
                Ok(_) => return Ok(()),
                Err(_) => continue,
            }
        }
    }

    Err(format!(
        "Could not find a terminal emulator. Run manually:\n  tmux attach-session -t {}",
        session_name
    ))
}

/// Detach/stop a worker
///
/// Stops the worker process and marks it as paused.
#[tauri::command]
pub async fn detach_worker(run_name: String, worker_id: u32) -> Result<(), String> {
    use crate::core::state::WorkerUpdate;
    use crate::core::workers::is_pid_alive;

    let state = get_run_state(&run_name)?;

    // Find the worker by ID
    let workers = state
        .get_workers()
        .map_err(|e| format!("Failed to get workers: {}", e))?;

    let worker = workers
        .iter()
        .find(|w| w.id as u32 == worker_id)
        .ok_or_else(|| format!("Worker with ID {} not found", worker_id))?;

    // Kill the process if it's running
    if let Some(pid) = worker.pid {
        if is_pid_alive(pid as u32) {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            tracing::info!("Stopped worker {} (PID {})", worker.name, pid);
        }
    }

    // Mark as paused
    state
        .update_worker(
            &worker.name,
            WorkerUpdate {
                pid: None,
                status: Some(crate::core::state::WorkerStatus::Paused),
                ..Default::default()
            },
        )
        .map_err(|e| format!("Failed to update worker: {}", e))?;

    tracing::info!("Detached worker {} from run {}", worker.name, run_name);
    Ok(())
}

/// Restart a worker
///
/// Stops the current worker process and spawns a new one.
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn restart_worker(run_name: String, worker_id: u32) -> Result<(), String> {
    let orch = create_orchestrator(None).map_err(err_string)?;

    // Get workers to find the worker name from the ID
    let workers = orch.list_workers(&run_name).await.map_err(err_string)?;

    let worker = workers
        .iter()
        .find(|w| w.id == worker_id)
        .ok_or_else(|| format!("Worker with ID {} not found", worker_id))?;

    // Call restart_worker with the worker name
    orch.restart_worker(&run_name, &worker.name)
        .await
        .map_err(err_string)
}
