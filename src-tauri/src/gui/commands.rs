//! Tauri IPC commands for the Hirsel GUI
//!
//! These commands provide the interface between the frontend and backend.
//! Types are designed to match the TypeScript definitions in src/lib/types.ts.

use serde::{Deserialize, Serialize};
use crate::core::{config, state::SQLiteState};

// =============================================================================
// Status Enums (match TypeScript types)
// =============================================================================

/// Run status values matching TypeScript RunStatus
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Idle,
    Working,
    Paused,
    Runaway,
    TimedOut,
    Eval,
    EvalFailed,
    Waiting,
    Done,
    Delivered,
    Merged,
}

/// Task status values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    Doing,
    Done,
}

/// Worker status values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Idle,
    Working,
    Waiting,
    Awaiting,
    Paused,
    Done,
    Error,
}

/// Worker location
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerLocation {
    Local,
    Remote,
}

/// Eval status values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalStatus {
    Running,
    Passed,
    Failed,
}

// =============================================================================
// Response Types (match TypeScript interfaces)
// =============================================================================

/// Summary of a run for the run list panel
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub name: String,
    pub status: RunStatus,
    pub tasks_done: u32,
    pub tasks_total: u32,
    pub workers_active: u32,
    pub workers_total: u32,
    pub elapsed_minutes: f64,
    pub time_limit_minutes: Option<u32>,
    pub has_unread_messages: bool,
}

/// Full run details for the detail view
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDetail {
    pub name: String,
    pub status: RunStatus,
    pub request: Option<String>,
    pub project_path: Option<String>,
    pub worker_scale: Option<String>,
    pub time_limit_minutes: Option<u32>,
    pub started_at: Option<String>,
    pub summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub iteration_count: u32,
    pub max_iterations: Option<u32>,
    pub human_in_the_loop: bool,
    pub waiting_reason: Option<String>,
    pub unread_count: u32,
}

/// Task from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub parent_id: Option<String>,
    pub blocked_by: Option<Vec<String>>,
    pub tokens_used: Option<u64>,
    pub created_at: String,
    pub pending_done_at: Option<String>,
}

/// Worker from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worker {
    pub id: u32,
    pub name: String,
    pub pid: Option<u32>,
    pub session_id: Option<String>,
    pub status: WorkerStatus,
    pub work_dir: Option<String>,
    pub waiting_thread: Option<String>,
    pub location: WorkerLocation,
    pub last_heartbeat: Option<String>,
    pub created_at: String,
    pub needs_restart: bool,
    pub session_started_at: Option<String>,
    // Display fields
    pub is_leader: bool,
    pub context_utilization: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub turns: Option<u32>,
    pub current_task: Option<String>,
}

/// Message from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: u32,
    pub thread: String,
    pub sender: String,
    pub content: String,
    pub waiting: bool,
    pub read_by: Option<Vec<String>>,
    pub timestamp: String,
}

/// Thread summary for chat panel
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub name: String,
    pub message_count: u32,
    pub unread_count: u32,
    pub last_message: Option<String>,
    pub last_timestamp: Option<String>,
}

/// History entry for activity log
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: u32,
    pub timestamp: String,
    pub action: String,
    pub detail: Option<String>,
}

/// Eval from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Eval {
    pub id: u32,
    pub branch: String,
    pub eval_name: Option<String>,
    pub status: EvalStatus,
    pub feedback: Option<String>,
    pub log_file: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

/// Agent preset configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPreset {
    pub name: String,
    pub command: Vec<String>,
    pub mcp_config: Option<serde_json::Value>,
}

/// Application configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigResponse {
    pub runs_dir: String,
    pub agent: String,
    pub agent_presets: std::collections::HashMap<String, AgentPreset>,
    pub default_worker_scale: String,
    pub default_time_limit: Option<u32>,
}

// =============================================================================
// Run Commands
// =============================================================================

/// Get list of all runs
#[tauri::command]
pub async fn get_runs() -> Result<Vec<RunSummary>, String> {
    let run_names = config::list_runs().map_err(|e| format!("list_runs error: {}", e))?;
    let mut runs = Vec::new();

    for name in run_names {
        let db_path = config::run_dir(&name).join("hirsel.db");
        if !db_path.exists() {
            continue;
        }

        match SQLiteState::new(db_path.clone()) {
            Ok(state) => {
                let status = state.status().unwrap_or(crate::core::state::Status::Idle);
                let tasks = state.get_tasks().unwrap_or_default();
                let workers = state.get_workers().unwrap_or_default();

                let tasks_done = tasks.iter().filter(|t| t.status == crate::core::state::TaskStatus::Done).count() as u32;
                let tasks_total = tasks.len() as u32;
                let workers_active = workers.iter().filter(|w| {
                    w.status == crate::core::state::WorkerStatus::Working
                }).count() as u32;
                let workers_total = workers.len() as u32;

                // Convert core Status to GUI RunStatus
                let run_status = match status {
                    crate::core::state::Status::Idle => RunStatus::Idle,
                    crate::core::state::Status::Working => RunStatus::Working,
                    crate::core::state::Status::Paused => RunStatus::Paused,
                    crate::core::state::Status::Runaway => RunStatus::Runaway,
                    crate::core::state::Status::TimedOut => RunStatus::TimedOut,
                    crate::core::state::Status::Eval => RunStatus::Eval,
                    crate::core::state::Status::EvalFailed => RunStatus::EvalFailed,
                    crate::core::state::Status::Waiting => RunStatus::Waiting,
                    crate::core::state::Status::Done => RunStatus::Done,
                    crate::core::state::Status::Delivered => RunStatus::Delivered,
                    crate::core::state::Status::Merged => RunStatus::Merged,
                };

                runs.push(RunSummary {
                    name,
                    status: run_status,
                    tasks_done,
                    tasks_total,
                    workers_active,
                    workers_total,
                    elapsed_minutes: 0.0, // TODO: calculate from started_at
                    time_limit_minutes: None,
                    has_unread_messages: false, // TODO: check messages
                });
            }
            Err(_) => continue,
        }
    }

    Ok(runs)
}

/// Get detailed information about a specific run
#[tauri::command]
pub async fn get_run_detail(run_name: String) -> Result<RunDetail, String> {
    // TODO: Integrate with state.rs
    Err(format!("Run '{}' not found", run_name))
}

/// Pause a running run
#[tauri::command]
pub async fn pause_run(run_name: String) -> Result<(), String> {
    // TODO: Integrate with state.rs to update run status
    eprintln!("[INFO] Pausing run: {}", run_name);
    Ok(())
}

/// Resume a paused run
#[tauri::command]
pub async fn resume_run(run_name: String) -> Result<(), String> {
    // TODO: Integrate with state.rs to update run status
    eprintln!("[INFO] Resuming run: {}", run_name);
    Ok(())
}

/// Delete a run
#[tauri::command]
pub async fn delete_run(run_name: String) -> Result<(), String> {
    // TODO: Integrate with state.rs to delete run
    eprintln!("[INFO] Deleting run: {}", run_name);
    Ok(())
}

// =============================================================================
// Task Commands
// =============================================================================

/// Get all tasks for a run
#[tauri::command]
pub async fn get_tasks(run_name: String) -> Result<Vec<Task>, String> {
    // TODO: Integrate with state.rs
    let _ = run_name;
    Ok(vec![])
}

/// Add a new task
#[tauri::command]
pub async fn add_task(
    run_name: String,
    task_id: String,
    description: String,
    parent_id: Option<String>,
    blocked_by: Option<Vec<String>>,
) -> Result<Task, String> {
    eprintln!(
        "[INFO] Adding task {} to run {}: {}",
        task_id, run_name, description
    );
    // TODO: Integrate with state.rs
    Ok(Task {
        id: task_id,
        description,
        status: TaskStatus::Todo,
        claimed_by: None,
        claimed_at: None,
        parent_id,
        blocked_by,
        tokens_used: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        pending_done_at: None,
    })
}

/// Delete a task
#[tauri::command]
pub async fn delete_task(run_name: String, task_id: String) -> Result<(), String> {
    eprintln!("[INFO] Deleting task {} from run {}", task_id, run_name);
    // TODO: Integrate with state.rs
    Ok(())
}

/// Mark a task as complete
#[tauri::command]
pub async fn complete_task(run_name: String, task_id: String) -> Result<(), String> {
    eprintln!(
        "[INFO] Marking task {} as complete in run {}",
        task_id, run_name
    );
    // TODO: Integrate with state.rs
    Ok(())
}

/// Unclaim a task (release it back to the pool)
#[tauri::command]
pub async fn unclaim_task(run_name: String, task_id: String) -> Result<(), String> {
    eprintln!("[INFO] Unclaiming task {} in run {}", task_id, run_name);
    // TODO: Integrate with state.rs
    Ok(())
}

/// Reopen a completed task
#[tauri::command]
pub async fn reopen_task(run_name: String, task_id: String) -> Result<(), String> {
    eprintln!("[INFO] Reopening task {} in run {}", task_id, run_name);
    // TODO: Integrate with state.rs
    Ok(())
}

// =============================================================================
// Worker Commands
// =============================================================================

/// Get all workers for a run
#[tauri::command]
pub async fn get_workers(run_name: String) -> Result<Vec<Worker>, String> {
    // TODO: Integrate with state.rs
    let _ = run_name;
    Ok(vec![])
}

/// Attach a new worker to a run
#[tauri::command]
pub async fn attach_worker(run_name: String, worker_name: String) -> Result<Worker, String> {
    eprintln!(
        "[INFO] Attaching worker {} to run {}",
        worker_name, run_name
    );
    // TODO: Integrate with state.rs and spawn actual worker
    Ok(Worker {
        id: 1,
        name: worker_name,
        pid: None,
        session_id: None,
        status: WorkerStatus::Idle,
        work_dir: None,
        waiting_thread: None,
        location: WorkerLocation::Local,
        last_heartbeat: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        needs_restart: false,
        session_started_at: None,
        is_leader: false,
        context_utilization: None,
        input_tokens: None,
        output_tokens: None,
        turns: None,
        current_task: None,
    })
}

/// Detach/stop a worker
#[tauri::command]
pub async fn detach_worker(run_name: String, worker_id: u32) -> Result<(), String> {
    eprintln!(
        "[INFO] Detaching worker {} from run {}",
        worker_id, run_name
    );
    // TODO: Integrate with state.rs
    Ok(())
}

/// Restart a worker
#[tauri::command]
pub async fn restart_worker(run_name: String, worker_id: u32) -> Result<(), String> {
    eprintln!(
        "[INFO] Restarting worker {} in run {}",
        worker_id, run_name
    );
    // TODO: Integrate with state.rs
    Ok(())
}

// =============================================================================
// Message Commands
// =============================================================================

/// Get messages for a thread
#[tauri::command]
pub async fn get_messages(
    run_name: String,
    thread_name: String,
    _limit: Option<u32>,
) -> Result<Vec<Message>, String> {
    // TODO: Integrate with state.rs
    let _ = (run_name, thread_name);
    Ok(vec![])
}

/// Get all threads for a run
#[tauri::command]
pub async fn get_threads(run_name: String) -> Result<Vec<ThreadSummary>, String> {
    // TODO: Integrate with state.rs
    let _ = run_name;
    Ok(vec![])
}

/// Send a message to a thread
#[tauri::command]
pub async fn send_message(
    run_name: String,
    thread_name: String,
    content: String,
) -> Result<Message, String> {
    eprintln!(
        "[INFO] Sending message to {}/{}: {}",
        run_name, thread_name, content
    );
    // TODO: Integrate with state.rs
    Ok(Message {
        id: 1,
        thread: thread_name,
        sender: "user".to_string(),
        content,
        waiting: false,
        read_by: None,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Mark messages as read
#[tauri::command]
pub async fn mark_messages_read(
    run_name: String,
    thread_name: String,
    reader: String,
) -> Result<(), String> {
    eprintln!(
        "[INFO] Marking messages read by {} in {}/{}",
        reader, run_name, thread_name
    );
    // TODO: Integrate with state.rs
    Ok(())
}

// =============================================================================
// History Commands
// =============================================================================

/// Get history entries for a run
#[tauri::command]
pub async fn get_history(run_name: String, _limit: Option<u32>) -> Result<Vec<HistoryEntry>, String> {
    // TODO: Integrate with state.rs
    let _ = run_name;
    Ok(vec![])
}

// =============================================================================
// Eval Commands
// =============================================================================

/// Get evals for a run
#[tauri::command]
pub async fn get_evals(run_name: String) -> Result<Vec<Eval>, String> {
    // TODO: Integrate with state.rs
    let _ = run_name;
    Ok(vec![])
}

/// Start an eval
#[tauri::command]
pub async fn start_eval(run_name: String, eval_name: Option<String>) -> Result<Eval, String> {
    eprintln!(
        "[INFO] Starting eval {:?} for run {}",
        eval_name, run_name
    );
    // TODO: Integrate with state.rs
    Ok(Eval {
        id: 1,
        branch: "main".to_string(),
        eval_name,
        status: EvalStatus::Running,
        feedback: None,
        log_file: None,
        started_at: chrono::Utc::now().to_rfc3339(),
        finished_at: None,
    })
}

// =============================================================================
// Config Commands
// =============================================================================

/// Get application configuration
#[tauri::command]
pub async fn get_config() -> Result<ConfigResponse, String> {
    // TODO: Load actual config from ~/.hirsel/config.toml using core::config
    let runs_dir = dirs::home_dir()
        .map(|h| h.join(".hirsel").join("runs").to_string_lossy().to_string())
        .unwrap_or_else(|| "~/.hirsel/runs".to_string());

    Ok(ConfigResponse {
        runs_dir,
        agent: "claude-code".to_string(),
        agent_presets: std::collections::HashMap::new(),
        default_worker_scale: "1".to_string(),
        default_time_limit: None,
    })
}

// =============================================================================
// Handler Registration
// =============================================================================

/// Generate the Tauri invoke handler with all commands
pub fn get_handlers() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        // Run commands
        get_runs,
        get_run_detail,
        pause_run,
        resume_run,
        delete_run,
        // Task commands
        get_tasks,
        add_task,
        delete_task,
        complete_task,
        unclaim_task,
        reopen_task,
        // Worker commands
        get_workers,
        attach_worker,
        detach_worker,
        restart_worker,
        // Message commands
        get_messages,
        get_threads,
        send_message,
        mark_messages_read,
        // History commands
        get_history,
        // Eval commands
        get_evals,
        start_eval,
        // Config commands
        get_config,
    ]
}
