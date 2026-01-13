//! Worker subprocess main loop implementation.
//!
//! The WorkerRunner manages the lifecycle of a single worker subprocess:
//! - Initializes connection to run state
//! - Spawns and communicates with the AI agent
//! - Handles task claim/done cycle
//! - Manages heartbeats and status updates
//! - Coordinates with other workers via messaging

use crate::cli::{MsgSubcommands, TaskSubcommands, WorkerCommands};
use crate::core::state::{SQLiteState, StateError, WorkerStatus, WorkerUpdate};
use crate::core::Files;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Errors that can occur during worker operations.
#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("Run directory not found: {0}")]
    RunNotFound(PathBuf),

    #[error("Worker not registered: {0}")]
    WorkerNotRegistered(String),

    #[error("State error: {0}")]
    State(#[from] StateError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("No task claimed")]
    NoTaskClaimed,

    #[error("Task already claimed: {0}")]
    TaskAlreadyClaimed(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

/// Result type for worker operations.
pub type WorkerResult<T> = Result<T, WorkerError>;

/// Configuration for a worker subprocess.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Name of this worker (e.g., "achilles", "ajax").
    pub worker_name: String,
    /// Path to the run directory.
    pub run_dir: PathBuf,
    /// Heartbeat interval in seconds.
    pub heartbeat_interval: u64,
}

impl WorkerConfig {
    /// Create a new worker configuration from environment variables.
    ///
    /// Expects HIRSEL_RUN and HIRSEL_WORKER environment variables.
    pub fn from_env() -> WorkerResult<Self> {
        let run_name = std::env::var("HIRSEL_RUN")
            .map_err(|_| WorkerError::Config("HIRSEL_RUN not set".into()))?;

        let worker_name = std::env::var("HIRSEL_WORKER")
            .map_err(|_| WorkerError::Config("HIRSEL_WORKER not set".into()))?;

        // Get runs directory from HIRSEL_ROOT or default
        let hirsel_root = std::env::var("HIRSEL_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".hirsel")
            });

        let run_dir = hirsel_root.join("runs").join(&run_name);

        if !run_dir.exists() {
            return Err(WorkerError::RunNotFound(run_dir));
        }

        Ok(Self {
            worker_name,
            run_dir,
            heartbeat_interval: 30,
        })
    }

    /// Create a worker configuration with explicit values.
    pub fn new(worker_name: String, run_dir: PathBuf) -> Self {
        Self {
            worker_name,
            run_dir,
            heartbeat_interval: 30,
        }
    }
}

/// The main worker subprocess runner.
pub struct WorkerRunner {
    config: WorkerConfig,
    state: SQLiteState,
    files: Files,
    last_heartbeat: Instant,
}

impl WorkerRunner {
    /// Create a new worker runner.
    pub fn new(config: WorkerConfig) -> WorkerResult<Self> {
        let files = Files::new(&config.run_dir);
        let state = SQLiteState::new(files.db_path()).map_err(WorkerError::State)?;

        // Verify worker exists in database
        let workers = state.get_workers().map_err(WorkerError::State)?;
        if !workers.iter().any(|w| w.name == config.worker_name) {
            return Err(WorkerError::WorkerNotRegistered(config.worker_name.clone()));
        }

        Ok(Self {
            config,
            state,
            files,
            last_heartbeat: Instant::now(),
        })
    }

    /// Get the worker name.
    pub fn worker_name(&self) -> &str {
        &self.config.worker_name
    }

    /// Get the run directory.
    pub fn run_dir(&self) -> &PathBuf {
        &self.config.run_dir
    }

    /// Get access to the state for direct queries.
    pub fn state(&self) -> &SQLiteState {
        &self.state
    }

    /// Update worker heartbeat in the database.
    pub fn heartbeat(&mut self) -> WorkerResult<()> {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.6f").to_string();
        self.state
            .update_worker(
                &self.config.worker_name,
                WorkerUpdate {
                    last_heartbeat: Some(now),
                    ..Default::default()
                },
            )
            .map_err(WorkerError::State)?;
        self.last_heartbeat = Instant::now();
        Ok(())
    }

    /// Check if heartbeat is due.
    pub fn heartbeat_due(&self) -> bool {
        self.last_heartbeat.elapsed() >= Duration::from_secs(self.config.heartbeat_interval)
    }

    /// Set worker status.
    pub fn set_status(&self, status: WorkerStatus) -> WorkerResult<()> {
        self.state
            .update_worker(
                &self.config.worker_name,
                WorkerUpdate {
                    status: Some(status),
                    ..Default::default()
                },
            )
            .map_err(WorkerError::State)
    }

    // =========================================================================
    // Task Operations
    // =========================================================================

    /// List all tasks.
    pub fn task_list(&self) -> WorkerResult<String> {
        let tasks = self.state.get_tasks().map_err(WorkerError::State)?;

        let output = serde_json::json!({
            "tasks": tasks.iter().map(|t| {
                let blocked = self.state.is_task_blocked(&t.id).unwrap_or(false);
                serde_json::json!({
                    "id": t.id,
                    "name": t.name,
                    "status": t.status.as_str(),
                    "claimed_by": t.claimed_by,
                    "parent": t.parent_id,
                    "blocked_by": t.blocked_by,
                    "blocked": blocked,
                })
            }).collect::<Vec<_>>()
        });

        serde_json::to_string_pretty(&output)
            .map_err(|e| WorkerError::Config(format!("Serialization error: {}", e)))
    }

    /// Claim a task.
    pub fn task_claim(&self, task_id: &str) -> WorkerResult<String> {
        self.state
            .claim_task(task_id, &self.config.worker_name)
            .map_err(WorkerError::State)?;

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
            "claimed_by": self.config.worker_name,
        })
        .to_string())
    }

    /// Mark a task as done.
    pub fn task_done(&self, task_id: Option<&str>) -> WorkerResult<String> {
        let tid = match task_id {
            Some(id) => id.to_string(),
            None => {
                // Get currently claimed task
                let task = self
                    .state
                    .get_claimed_task(&self.config.worker_name)
                    .map_err(WorkerError::State)?
                    .ok_or(WorkerError::NoTaskClaimed)?;
                task.id
            }
        };

        self.state
            .complete_task(&tid, &self.config.worker_name)
            .map_err(WorkerError::State)?;

        Ok(serde_json::json!({
            "success": true,
            "task_id": tid,
        })
        .to_string())
    }

    /// Unclaim a task.
    pub fn task_unclaim(&self, task_id: Option<&str>) -> WorkerResult<String> {
        let tid = match task_id {
            Some(id) => id.to_string(),
            None => {
                let task = self
                    .state
                    .get_claimed_task(&self.config.worker_name)
                    .map_err(WorkerError::State)?
                    .ok_or(WorkerError::NoTaskClaimed)?;
                task.id
            }
        };

        self.state
            .unclaim_task(&tid, &self.config.worker_name)
            .map_err(WorkerError::State)?;

        Ok(serde_json::json!({
            "success": true,
            "task_id": tid,
        })
        .to_string())
    }

    /// Add a new task.
    pub fn task_add(
        &self,
        task_id: &str,
        name: &str,
        parent: Option<&str>,
        blocked_by: &[String],
    ) -> WorkerResult<String> {
        let blocked_refs: Vec<&str> = blocked_by.iter().map(|s| s.as_str()).collect();

        self.state
            .add_task(
                task_id,
                name,
                parent,
                if blocked_refs.is_empty() {
                    None
                } else {
                    Some(blocked_refs.as_slice())
                },
            )
            .map_err(WorkerError::State)?;

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
        })
        .to_string())
    }

    /// Delete a task.
    pub fn task_delete(&self, task_id: &str) -> WorkerResult<String> {
        self.state
            .delete_task(task_id)
            .map_err(WorkerError::State)?;

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
        })
        .to_string())
    }

    /// Reopen a completed task.
    pub fn task_undone(&self, task_id: &str) -> WorkerResult<String> {
        self.state
            .reopen_task(task_id)
            .map_err(WorkerError::State)?;

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
        })
        .to_string())
    }

    /// Wait for tasks to become available.
    pub fn task_await(&self) -> WorkerResult<String> {
        // Set worker status to awaiting
        self.set_status(WorkerStatus::Awaiting)?;

        // Check for available tasks
        let claimable = self
            .state
            .get_claimable_tasks()
            .map_err(WorkerError::State)?;

        Ok(serde_json::json!({
            "available_tasks": claimable.len(),
            "tasks": claimable.iter().map(|t| serde_json::json!({
                "id": t.id,
                "name": t.name,
            })).collect::<Vec<_>>(),
        })
        .to_string())
    }

    // =========================================================================
    // Message Operations
    // =========================================================================

    /// Send a message to a thread.
    pub fn msg_send(&self, thread: &str, message: &str, wait: bool) -> WorkerResult<String> {
        self.state
            .add_message(thread, &self.config.worker_name, message, wait)
            .map_err(WorkerError::State)?;

        if wait {
            self.set_status(WorkerStatus::Waiting)?;
        }

        Ok(serde_json::json!({
            "success": true,
            "thread": thread,
            "waiting": wait,
        })
        .to_string())
    }

    /// Read messages from a thread (or all threads).
    pub fn msg_read(&self, thread: Option<&str>) -> WorkerResult<String> {
        let messages = if let Some(t) = thread {
            self.state
                .get_unread_messages(t, &self.config.worker_name)
                .map_err(WorkerError::State)?
        } else {
            // Read from all threads
            self.state
                .get_all_unread_messages(&self.config.worker_name)
                .map_err(WorkerError::State)?
        };

        Ok(serde_json::json!({
            "messages": messages.iter().map(|m| serde_json::json!({
                "thread": m.thread,
                "sender": m.sender,
                "content": m.content,
                "timestamp": m.timestamp,
                "waiting": m.waiting,
            })).collect::<Vec<_>>(),
        })
        .to_string())
    }

    /// List available message threads.
    pub fn msg_list(&self) -> WorkerResult<String> {
        let threads = self.state.get_threads().map_err(WorkerError::State)?;

        Ok(serde_json::json!({
            "threads": threads,
        })
        .to_string())
    }

    /// Check inbox for new messages.
    pub fn msg_inbox(&self) -> WorkerResult<String> {
        let threads = self.state.get_threads().map_err(WorkerError::State)?;

        let mut inbox = Vec::new();
        for thread in &threads {
            let messages = self
                .state
                .get_unread_messages(thread, &self.config.worker_name)
                .map_err(WorkerError::State)?;

            if !messages.is_empty() {
                inbox.push(serde_json::json!({
                    "thread": thread,
                    "count": messages.len(),
                    "messages": messages.iter().map(|m| serde_json::json!({
                        "sender": m.sender,
                        "content": m.content,
                        "timestamp": m.timestamp,
                    })).collect::<Vec<_>>(),
                }));
            }
        }

        Ok(serde_json::json!({
            "inbox": inbox,
        })
        .to_string())
    }

    // =========================================================================
    // Work Done
    // =========================================================================

    /// Signal that all work is complete.
    pub fn work_done(&self) -> WorkerResult<String> {
        // Mark worker as done (this is logged via worker status tracking)
        self.set_status(WorkerStatus::Done)?;

        Ok(serde_json::json!({
            "success": true,
            "worker": self.config.worker_name,
        })
        .to_string())
    }

    // =========================================================================
    // Command Execution
    // =========================================================================

    /// Execute a worker command and return JSON output.
    pub fn execute_command(&mut self, command: WorkerCommands) -> WorkerResult<String> {
        // Update heartbeat on each command
        if self.heartbeat_due() {
            self.heartbeat()?;
        }

        match command {
            WorkerCommands::Done => self.work_done(),

            WorkerCommands::Task(task_cmd) => match task_cmd {
                TaskSubcommands::List => self.task_list(),
                TaskSubcommands::Add(args) => self.task_add(
                    &args.task_id,
                    &args.name,
                    args.parent.as_deref(),
                    &args.blocked_by,
                ),
                TaskSubcommands::Claim(args) => self.task_claim(&args.task_id),
                TaskSubcommands::Done(args) => self.task_done(args.task_id.as_deref()),
                TaskSubcommands::Unclaim(args) => self.task_unclaim(args.task_id.as_deref()),
                TaskSubcommands::Undone(args) => self.task_undone(&args.task_id),
                TaskSubcommands::Delete(args) => self.task_delete(&args.task_id),
                TaskSubcommands::Await => self.task_await(),
            },

            WorkerCommands::Msg(msg_cmd) => match msg_cmd {
                MsgSubcommands::Send(args) => self.msg_send(&args.thread, &args.message, args.wait),
                MsgSubcommands::Read(args) => self.msg_read(args.thread.as_deref()),
                MsgSubcommands::List => self.msg_list(),
                MsgSubcommands::Inbox => self.msg_inbox(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_config_new() {
        let config = WorkerConfig::new("achilles".into(), PathBuf::from("/tmp/test-run"));
        assert_eq!(config.worker_name, "achilles");
        assert_eq!(config.heartbeat_interval, 30);
    }

    #[test]
    fn test_worker_config_from_env_missing_vars() {
        // Clear env vars to ensure they're not set
        std::env::remove_var("HIRSEL_RUN");
        std::env::remove_var("HIRSEL_WORKER");

        let result = WorkerConfig::from_env();
        assert!(matches!(result, Err(WorkerError::Config(_))));
    }
}
