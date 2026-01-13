//! Tauri IPC commands for the Hirsel GUI
//!
//! These commands provide the interface between the frontend and backend.
//! Types are designed to match the TypeScript definitions in src/lib/types.ts.

use chrono::{NaiveDateTime, Utc, TimeZone};
use serde::{Deserialize, Serialize};
use crate::core::{config, metrics, state::SQLiteState};

/// Parse a timestamp string (with or without timezone) and return elapsed minutes
fn parse_elapsed_minutes(timestamp: &str) -> f64 {
    // Try RFC3339 first (has timezone)
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(timestamp) {
        let now = Utc::now();
        let elapsed = now.signed_duration_since(dt.with_timezone(&Utc));
        return elapsed.num_seconds() as f64 / 60.0;
    }

    // Try parsing as NaiveDateTime (no timezone, assume UTC)
    // Format: "2024-01-13T12:30:45.123456"
    if let Ok(naive) = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%S%.f") {
        let dt = Utc.from_utc_datetime(&naive);
        let now = Utc::now();
        let elapsed = now.signed_duration_since(dt);
        return elapsed.num_seconds() as f64 / 60.0;
    }

    // Try without fractional seconds
    if let Ok(naive) = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%S") {
        let dt = Utc.from_utc_datetime(&naive);
        let now = Utc::now();
        let elapsed = now.signed_duration_since(dt);
        return elapsed.num_seconds() as f64 / 60.0;
    }

    0.0
}

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
    pub created_at: String,
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
    // Additional fields for status bar display
    pub tasks_done: u32,
    pub tasks_total: u32,
    pub workers_active: u32,
    pub workers_total: u32,
    pub elapsed_minutes: f64,
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
    pub agent_command: Vec<String>,
    pub eval_timeout: u32,
    pub auto_learn: bool,
    pub max_iterations: Option<u32>,
    pub user_message_pause: String,
    pub human_in_the_loop: bool,
    pub compaction_threshold: Option<u32>,
    pub compaction_keep_messages: u32,
    pub context_warning_threshold: f64,
    pub coordinator_port: u16,
}

/// Request to update configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigUpdateRequest {
    pub agent_command: Option<Vec<String>>,
    pub eval_timeout: Option<u32>,
    pub auto_learn: Option<bool>,
    pub max_iterations: Option<Option<u32>>,
    pub user_message_pause: Option<String>,
    pub human_in_the_loop: Option<bool>,
    pub compaction_threshold: Option<Option<u32>>,
    pub compaction_keep_messages: Option<u32>,
    pub context_warning_threshold: Option<f64>,
    pub coordinator_port: Option<u16>,
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

                // Calculate elapsed minutes from started_at or created_at
                let elapsed_minutes = if let Ok(Some(time_info)) = state.get_time_info() {
                    time_info.elapsed_minutes
                } else if let Ok(Some(started_at)) = state.get_started_at() {
                    parse_elapsed_minutes(&started_at)
                } else if let Ok(Some(created_at)) = state.get_created_at() {
                    parse_elapsed_minutes(&created_at)
                } else {
                    0.0
                };

                // Get time limit
                let time_limit_minutes = state.get_time_limit_minutes()
                    .ok()
                    .flatten()
                    .map(|m| m as u32);

                // Check for unread messages
                let has_unread_messages = state.get_unread_count().unwrap_or(0) > 0;

                // Get created_at for sorting
                let created_at = state.get_created_at()
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| Utc::now().to_rfc3339());

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
                    elapsed_minutes,
                    time_limit_minutes,
                    has_unread_messages,
                    created_at,
                });
            }
            Err(_) => continue,
        }
    }

    // Sort by created_at descending (newest first)
    runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(runs)
}

/// Get detailed information about a specific run
#[tauri::command]
pub async fn get_run_detail(run_name: String) -> Result<RunDetail, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    let status = state.status().unwrap_or(crate::core::state::Status::Idle);
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

    let request = state.get_request().ok().flatten();
    let project_path = state.get_project_path().ok().flatten();
    let worker_scale = state.get_worker_scale().ok().flatten();
    let time_limit_minutes = state.get_time_limit_minutes().ok().flatten().map(|m| m as u32);
    let started_at = state.get_started_at().ok().flatten();
    let summary = state.get_summary().ok().flatten();
    let created_at = state.get_created_at().ok().flatten().unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let iteration_count = state.get_iteration_count().unwrap_or(0) as u32;
    let max_iterations = state.get_max_iterations().ok().flatten().map(|m| m as u32);
    let human_in_the_loop = state.get_human_in_the_loop().unwrap_or(true);
    let waiting_reason = state.get_waiting_reason().ok().flatten();
    let unread_count = state.get_unread_count().unwrap_or(0) as u32;

    // Get tasks and workers for counts
    let tasks = state.get_tasks().unwrap_or_default();
    let workers = state.get_workers().unwrap_or_default();

    let tasks_done = tasks.iter().filter(|t| t.status == crate::core::state::TaskStatus::Done).count() as u32;
    let tasks_total = tasks.len() as u32;
    let workers_active = workers.iter().filter(|w| {
        w.status == crate::core::state::WorkerStatus::Working
    }).count() as u32;
    let workers_total = workers.len() as u32;

    // Calculate elapsed minutes
    let elapsed_minutes = if let Ok(Some(time_info)) = state.get_time_info() {
        time_info.elapsed_minutes
    } else if let Some(ref sa) = started_at {
        parse_elapsed_minutes(sa)
    } else {
        parse_elapsed_minutes(&created_at)
    };

    Ok(RunDetail {
        name: run_name,
        status: run_status,
        request,
        project_path,
        worker_scale,
        time_limit_minutes,
        started_at,
        summary,
        created_at: created_at.clone(),
        updated_at: created_at, // TODO: Track updated_at separately
        iteration_count,
        max_iterations,
        human_in_the_loop,
        waiting_reason,
        unread_count,
        tasks_done,
        tasks_total,
        workers_active,
        workers_total,
        elapsed_minutes,
    })
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
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    let core_tasks = state.get_tasks()
        .map_err(|e| format!("Failed to get tasks: {}", e))?;

    let tasks = core_tasks.into_iter().map(|t| {
        let status = match t.status {
            crate::core::state::TaskStatus::Todo => TaskStatus::Todo,
            crate::core::state::TaskStatus::Doing => TaskStatus::Doing,
            crate::core::state::TaskStatus::Done => TaskStatus::Done,
        };

        // Parse blocked_by string into Vec<String>
        let blocked_by = t.blocked_by.as_ref().map(|b| {
            b.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        });

        Task {
            id: t.id,
            description: t.name,
            status,
            claimed_by: t.claimed_by,
            claimed_at: t.claimed_at,
            parent_id: t.parent_id,
            blocked_by,
            tokens_used: t.tokens_used.map(|n| n as u64),
            created_at: t.created_at,
            pending_done_at: t.pending_done_at,
        }
    }).collect();

    Ok(tasks)
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
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    let core_workers = state.get_workers()
        .map_err(|e| format!("Failed to get workers: {}", e))?;

    // Get tasks to find current task for each worker
    let tasks = state.get_tasks().unwrap_or_default();

    let workers = core_workers.into_iter().map(|w| {
        let status = match w.status {
            crate::core::state::WorkerStatus::Idle => WorkerStatus::Idle,
            crate::core::state::WorkerStatus::Working => WorkerStatus::Working,
            crate::core::state::WorkerStatus::Waiting => WorkerStatus::Waiting,
            crate::core::state::WorkerStatus::Awaiting => WorkerStatus::Awaiting,
            crate::core::state::WorkerStatus::Paused => WorkerStatus::Paused,
            crate::core::state::WorkerStatus::Error => WorkerStatus::Error,
        };

        let location = match w.location.as_str() {
            "remote" => WorkerLocation::Remote,
            _ => WorkerLocation::Local,
        };

        // Find current task for this worker
        let current_task = tasks.iter()
            .find(|t| t.claimed_by.as_deref() == Some(&w.name) && t.status == crate::core::state::TaskStatus::Doing)
            .map(|t| t.name.clone());

        // Check if this is the leader (first worker or worker id 1)
        let is_leader = w.id == 1;

        // Get session metrics for this worker
        let session_metrics = metrics::get_session_metrics(
            w.session_id.as_deref(),
            w.work_dir.as_deref(),
        );

        Worker {
            id: w.id as u32,
            name: w.name.clone(),
            pid: w.pid.map(|p| p as u32),
            session_id: w.session_id,
            status,
            work_dir: w.work_dir,
            waiting_thread: w.waiting_thread,
            location,
            last_heartbeat: w.last_heartbeat,
            created_at: w.created_at,
            needs_restart: w.needs_restart,
            session_started_at: w.session_started_at,
            is_leader,
            context_utilization: session_metrics.context_utilization,
            input_tokens: Some(session_metrics.input_tokens),
            output_tokens: Some(session_metrics.output_tokens),
            turns: Some(session_metrics.turns),
            current_task,
        }
    }).collect();

    Ok(workers)
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
    limit: Option<u32>,
) -> Result<Vec<Message>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    let limit = limit.unwrap_or(100) as i64;
    let core_messages = state.get_messages(&thread_name, limit)
        .map_err(|e| format!("Failed to get messages: {}", e))?;

    let messages = core_messages.into_iter().map(|m| {
        Message {
            id: m.id as u32,
            thread: m.thread,
            sender: m.sender,
            content: m.content,
            waiting: m.waiting,
            read_by: None, // TODO: Track read_by
            timestamp: m.timestamp,
        }
    }).collect();

    Ok(messages)
}

/// Get all threads for a run
#[tauri::command]
pub async fn get_threads(run_name: String) -> Result<Vec<ThreadSummary>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    let thread_names = state.get_threads()
        .map_err(|e| format!("Failed to get threads: {}", e))?;

    let mut threads = Vec::new();
    for name in thread_names {
        let message_count = state.get_thread_message_count(&name).unwrap_or(0) as u32;
        let messages = state.get_messages(&name, 1).unwrap_or_default();
        let last_message = messages.first().map(|m| m.content.clone());
        let last_timestamp = messages.first().map(|m| m.timestamp.clone());

        threads.push(ThreadSummary {
            name,
            message_count,
            unread_count: 0, // TODO: Calculate unread count per thread
            last_message,
            last_timestamp,
        });
    }

    Ok(threads)
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
pub async fn get_history(run_name: String, limit: Option<u32>) -> Result<Vec<HistoryEntry>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    let limit = limit.unwrap_or(100) as i64;
    let core_history = state.get_history(limit)
        .map_err(|e| format!("Failed to get history: {}", e))?;

    let history = core_history.into_iter().map(|h| {
        HistoryEntry {
            id: h.id as u32,
            timestamp: h.timestamp,
            action: h.action,
            detail: h.detail,
        }
    }).collect();

    Ok(history)
}

// =============================================================================
// Eval Commands
// =============================================================================

/// Get the eval spec (eval.md) content for a run
#[tauri::command]
pub async fn get_eval_spec(run_name: String) -> Result<Option<String>, String> {
    let run_dir = config::run_dir(&run_name);
    let eval_spec_path = run_dir.join("eval.md");

    if !eval_spec_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&eval_spec_path)
        .map_err(|e| format!("Failed to read eval spec: {}", e))?;

    Ok(Some(content))
}

/// Get evals for a run
#[tauri::command]
pub async fn get_evals(run_name: String) -> Result<Vec<Eval>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    let core_evals = state.get_evals(100)
        .map_err(|e| format!("Failed to get evals: {}", e))?;

    let evals = core_evals.into_iter().map(|e| {
        let status = match e.status {
            crate::core::state::EvalStatus::Running => EvalStatus::Running,
            crate::core::state::EvalStatus::Passed => EvalStatus::Passed,
            crate::core::state::EvalStatus::Failed => EvalStatus::Failed,
        };

        Eval {
            id: e.id as u32,
            branch: e.branch,
            eval_name: e.eval_name,
            status,
            feedback: e.feedback,
            log_file: e.log_file,
            started_at: e.started_at,
            finished_at: e.finished_at,
        }
    }).collect();

    Ok(evals)
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
// Worker Log Commands
// =============================================================================

/// Response for worker log content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerLogResponse {
    pub content: String,
    pub byte_offset: u64,
    pub file_size: u64,
    pub exists: bool,
}

/// Parsed log line with tool activity info
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedLogLine {
    pub text: String,
    pub is_tool_start: bool,
    pub is_tool_end: bool,
    pub tool_name: Option<String>,
}

/// Get worker log file content
///
/// Returns the log content for a specific worker. Supports optional line limit
/// and byte offset for efficient tailing/streaming.
#[tauri::command]
pub async fn get_worker_log(
    run_name: String,
    worker_name: String,
    lines: Option<u32>,
    from_offset: Option<u64>,
) -> Result<WorkerLogResponse, String> {
    use std::io::{Read, Seek, SeekFrom};

    let run_dir = config::run_dir(&run_name);
    let files = crate::core::Files::new(&run_dir);
    let log_path = files.worker_log(&worker_name);

    if !log_path.exists() {
        return Ok(WorkerLogResponse {
            content: String::new(),
            byte_offset: 0,
            file_size: 0,
            exists: false,
        });
    }

    let metadata = std::fs::metadata(&log_path)
        .map_err(|e| format!("Failed to read log file metadata: {}", e))?;
    let file_size = metadata.len();

    let mut file = std::fs::File::open(&log_path)
        .map_err(|e| format!("Failed to open log file: {}", e))?;

    // If offset is provided, seek to that position
    let start_offset = if let Some(offset) = from_offset {
        if offset < file_size {
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| format!("Failed to seek in log file: {}", e))?;
            offset
        } else {
            // Already at or past end
            return Ok(WorkerLogResponse {
                content: String::new(),
                byte_offset: file_size,
                file_size,
                exists: true,
            });
        }
    } else {
        0
    };

    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| format!("Failed to read log file: {}", e))?;

    // If lines limit is specified and no offset was given, return only the last N lines
    if lines.is_some() && from_offset.is_none() {
        let limit = lines.unwrap() as usize;
        let all_lines: Vec<&str> = content.lines().collect();
        if all_lines.len() > limit {
            content = all_lines[all_lines.len() - limit..].join("\n");
        }
    }

    Ok(WorkerLogResponse {
        content,
        byte_offset: file_size,
        file_size,
        exists: true,
    })
}

/// Get worker log file path
///
/// Returns the absolute path to the worker's log file for use with
/// file system watchers or external tools.
#[tauri::command]
pub async fn get_worker_log_path(
    run_name: String,
    worker_name: String,
) -> Result<String, String> {
    let run_dir = config::run_dir(&run_name);
    let files = crate::core::Files::new(&run_dir);
    let log_path = files.worker_log(&worker_name);
    Ok(log_path.to_string_lossy().to_string())
}

/// Parse log content and extract tool activity markers
///
/// Parses `[tool:name]` and `[/tool]` markers from Claude Code output.
#[tauri::command]
pub async fn parse_worker_log(content: String) -> Result<Vec<ParsedLogLine>, String> {
    let tool_start_re = regex::Regex::new(r"\[tool:([^\]]+)\]")
        .map_err(|e| format!("Invalid regex: {}", e))?;
    let tool_end_re = regex::Regex::new(r"\[/tool\]")
        .map_err(|e| format!("Invalid regex: {}", e))?;

    let parsed: Vec<ParsedLogLine> = content
        .lines()
        .map(|line| {
            let is_tool_start = tool_start_re.is_match(line);
            let is_tool_end = tool_end_re.is_match(line);
            let tool_name = if is_tool_start {
                tool_start_re.captures(line).map(|c| c[1].to_string())
            } else {
                None
            };

            ParsedLogLine {
                text: line.to_string(),
                is_tool_start,
                is_tool_end,
                tool_name,
            }
        })
        .collect();

    Ok(parsed)
}

/// Get eval log file content
#[tauri::command]
pub async fn get_eval_log(
    run_name: String,
    lines: Option<u32>,
    from_offset: Option<u64>,
) -> Result<WorkerLogResponse, String> {
    use std::io::{Read, Seek, SeekFrom};

    let run_dir = config::run_dir(&run_name);
    let files = crate::core::Files::new(&run_dir);
    let log_path = files.eval_log();

    if !log_path.exists() {
        return Ok(WorkerLogResponse {
            content: String::new(),
            byte_offset: 0,
            file_size: 0,
            exists: false,
        });
    }

    let metadata = std::fs::metadata(&log_path)
        .map_err(|e| format!("Failed to read eval log metadata: {}", e))?;
    let file_size = metadata.len();

    let mut file = std::fs::File::open(&log_path)
        .map_err(|e| format!("Failed to open eval log: {}", e))?;

    let start_offset = if let Some(offset) = from_offset {
        if offset < file_size {
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| format!("Failed to seek in eval log: {}", e))?;
            offset
        } else {
            return Ok(WorkerLogResponse {
                content: String::new(),
                byte_offset: file_size,
                file_size,
                exists: true,
            });
        }
    } else {
        0
    };

    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| format!("Failed to read eval log: {}", e))?;

    if lines.is_some() && from_offset.is_none() {
        let limit = lines.unwrap() as usize;
        let all_lines: Vec<&str> = content.lines().collect();
        if all_lines.len() > limit {
            content = all_lines[all_lines.len() - limit..].join("\n");
        }
    }

    Ok(WorkerLogResponse {
        content,
        byte_offset: file_size,
        file_size,
        exists: true,
    })
}

// =============================================================================
// Worker Events Commands (ACP-based streaming)
// =============================================================================

/// Worker event for real-time streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEventResponse {
    pub id: i64,
    pub worker_name: String,
    pub event_type: String,
    pub timestamp: String,
    /// Text content (for text/thought events)
    pub content: Option<String>,
    /// Tool call ID (for tool events)
    pub tool_call_id: Option<String>,
    /// Tool title/name
    pub tool_title: Option<String>,
    /// Tool kind (read, edit, execute, search, etc.)
    pub tool_kind: Option<String>,
    /// Tool execution status (pending, in_progress, completed, failed)
    pub tool_status: Option<String>,
    /// Tool input (JSON string)
    pub tool_input: Option<String>,
    /// Tool output (JSON string)
    pub tool_output: Option<String>,
}

/// Response for worker events query
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEventsResponse {
    pub events: Vec<WorkerEventResponse>,
    pub last_id: Option<i64>,
}

/// Get worker events for real-time streaming
///
/// Returns events since `after_id` for efficient polling.
/// On first call, pass `after_id: null` to get recent events.
#[tauri::command]
pub async fn get_worker_events(
    run_name: String,
    worker_name: String,
    after_id: Option<i64>,
    limit: Option<i64>,
) -> Result<WorkerEventsResponse, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Ok(WorkerEventsResponse {
            events: Vec::new(),
            last_id: None,
        });
    }

    let state = SQLiteState::new(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    let limit = limit.unwrap_or(1000);
    let events = state.get_worker_events(&worker_name, after_id, limit)
        .map_err(|e| format!("Failed to get worker events: {}", e))?;

    let last_id = events.last().map(|e| e.id);

    let events: Vec<WorkerEventResponse> = events.into_iter().map(|e| {
        WorkerEventResponse {
            id: e.id,
            worker_name: e.worker_name,
            event_type: e.event_type.as_str().to_string(),
            timestamp: e.timestamp,
            content: e.content,
            tool_call_id: e.tool_call_id,
            tool_title: e.tool_title,
            tool_kind: e.tool_kind,
            tool_status: e.tool_status.map(|s| s.as_str().to_string()),
            tool_input: e.tool_input,
            tool_output: e.tool_output,
        }
    }).collect();

    Ok(WorkerEventsResponse {
        events,
        last_id,
    })
}

/// Clear worker events (for cleanup when attaching/detaching)
#[tauri::command]
pub async fn clear_worker_events(
    run_name: String,
    worker_name: String,
) -> Result<(), String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Ok(());
    }

    let state = SQLiteState::new(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    state.clear_worker_events(&worker_name)
        .map_err(|e| format!("Failed to clear worker events: {}", e))?;

    Ok(())
}

// =============================================================================
// Config Commands
// =============================================================================

/// Get application configuration
#[tauri::command]
pub async fn get_config() -> Result<ConfigResponse, String> {
    let (cfg, _warnings) = config::Config::load()
        .map_err(|e| format!("Failed to load config: {}", e))?;

    let runs_dir = cfg.runs_dir().to_string_lossy().to_string();

    Ok(ConfigResponse {
        runs_dir,
        agent_command: cfg.agent.command,
        eval_timeout: cfg.eval_timeout,
        auto_learn: cfg.auto_learn,
        max_iterations: cfg.max_iterations,
        user_message_pause: cfg.user_message_pause,
        human_in_the_loop: cfg.human_in_the_loop,
        compaction_threshold: cfg.compaction_threshold,
        compaction_keep_messages: cfg.compaction_keep_messages,
        context_warning_threshold: cfg.context_warning_threshold,
        coordinator_port: cfg.coordinator_port,
    })
}

/// Save application configuration
#[tauri::command]
pub async fn save_config(updates: ConfigUpdateRequest) -> Result<(), String> {
    let config_path = config::hirsel_dir().join("config.toml");

    // Load existing config or create default
    let (mut cfg, _) = config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));

    // Apply updates
    if let Some(cmd) = updates.agent_command {
        cfg.agent.command = cmd;
    }
    if let Some(timeout) = updates.eval_timeout {
        cfg.eval_timeout = timeout;
    }
    if let Some(auto) = updates.auto_learn {
        cfg.auto_learn = auto;
    }
    if let Some(max) = updates.max_iterations {
        cfg.max_iterations = max;
    }
    if let Some(pause) = updates.user_message_pause {
        cfg.user_message_pause = pause;
    }
    if let Some(hitl) = updates.human_in_the_loop {
        cfg.human_in_the_loop = hitl;
    }
    if let Some(threshold) = updates.compaction_threshold {
        cfg.compaction_threshold = threshold;
    }
    if let Some(keep) = updates.compaction_keep_messages {
        cfg.compaction_keep_messages = keep;
    }
    if let Some(warning) = updates.context_warning_threshold {
        cfg.context_warning_threshold = warning;
    }
    if let Some(port) = updates.coordinator_port {
        cfg.coordinator_port = port;
    }

    // Serialize to TOML
    let toml_str = toml::to_string_pretty(&cfg)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    // Write to file
    std::fs::write(&config_path, toml_str)
        .map_err(|e| format!("Failed to write config: {}", e))?;

    Ok(())
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
        // Worker log commands
        get_worker_log,
        get_worker_log_path,
        parse_worker_log,
        get_eval_log,
        // Worker events commands (ACP-based streaming)
        get_worker_events,
        clear_worker_events,
        // Message commands
        get_messages,
        get_threads,
        send_message,
        mark_messages_read,
        // History commands
        get_history,
        // Eval commands
        get_eval_spec,
        get_evals,
        start_eval,
        // Config commands
        get_config,
        save_config,
    ]
}
