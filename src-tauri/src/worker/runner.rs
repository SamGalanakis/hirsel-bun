//! Worker subprocess main loop implementation.
//!
//! The WorkerRunner manages the lifecycle of a single worker subprocess:
//! - Initializes connection to run state
//! - Spawns and communicates with the AI agent
//! - Handles task claim/done cycle
//! - Manages heartbeats and status updates
//! - Coordinates with other workers via messaging
//!
//! Workers are state-agnostic - they don't know if they're accessing state
//! locally (SQLiteState) or remotely (HttpState). The HIRSEL_API_URL
//! environment variable determines which backend is used.

use crate::cli::{MsgSubcommands, TaskSubcommands, WorkerCommands};
use crate::core::state::{SQLiteState, StateError, WorkerStatus, WorkerUpdate};
use crate::core::state_access::{StateAccess, StateAccessError};
use crate::core::Files;
use crate::worker::http_state::HttpState;
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

    #[error("State access error: {0}")]
    StateAccess(#[from] StateAccessError),

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
    /// Name of the run.
    pub run_name: String,
    /// Path to the run directory.
    pub run_dir: PathBuf,
    /// Agent command to use for spawning workers.
    pub agent_command: Vec<String>,
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

        // Get agent command from environment or default
        let agent_command = std::env::var("HIRSEL_AGENT_COMMAND")
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| vec!["hirsel".to_string(), "__acp-bridge".to_string()]);

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
            run_name,
            run_dir,
            agent_command,
            heartbeat_interval: 30,
        })
    }

    /// Create a worker configuration with explicit values.
    pub fn new(
        worker_name: String,
        run_name: String,
        run_dir: PathBuf,
        agent_command: Vec<String>,
    ) -> Self {
        Self {
            worker_name,
            run_name,
            run_dir,
            agent_command,
            heartbeat_interval: 30,
        }
    }
}

/// State backend enum - either local SQLite or remote HTTP.
enum StateBackend {
    Local(SQLiteState),
    Remote {
        state: HttpState,
        runtime: tokio::runtime::Runtime,
    },
}

/// The main worker subprocess runner.
///
/// Workers are state-agnostic - they don't know if they're accessing state
/// locally or remotely. The backend is chosen based on HIRSEL_API_URL.
pub struct WorkerRunner {
    config: WorkerConfig,
    backend: StateBackend,
    _files: Option<Files>,
    last_heartbeat: Instant,
}

impl WorkerRunner {
    /// Create a new worker runner.
    ///
    /// If HIRSEL_API_URL is set, uses HttpState to communicate with a
    /// remote coordinator. Otherwise, uses SQLiteState for local access.
    pub fn new(config: WorkerConfig) -> WorkerResult<Self> {
        // Check if we should use remote state
        if let Ok(api_url) = std::env::var("HIRSEL_API_URL") {
            // Remote mode - use HttpState
            let runtime = tokio::runtime::Runtime::new()
                .map_err(|e| WorkerError::Config(format!("Failed to create runtime: {}", e)))?;

            let state = HttpState::new(&api_url, &config.worker_name, 30);

            // Verify connection and worker exists
            let workers = runtime
                .block_on(state.get_workers())
                .map_err(|e| WorkerError::StateAccess(e.into()))?;

            if !workers.iter().any(|w| w.name == config.worker_name) {
                return Err(WorkerError::WorkerNotRegistered(config.worker_name.clone()));
            }

            Ok(Self {
                config,
                backend: StateBackend::Remote { state, runtime },
                _files: None,
                last_heartbeat: Instant::now(),
            })
        } else {
            // Local mode - use SQLiteState
            let files = Files::new(&config.run_dir);
            let state = SQLiteState::new(files.db_path()).map_err(WorkerError::State)?;

            // Verify worker exists in database
            let workers = state.get_workers().map_err(WorkerError::State)?;
            if !workers.iter().any(|w| w.name == config.worker_name) {
                return Err(WorkerError::WorkerNotRegistered(config.worker_name.clone()));
            }

            Ok(Self {
                config,
                backend: StateBackend::Local(state),
                _files: Some(files),
                last_heartbeat: Instant::now(),
            })
        }
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
    /// Returns None if using remote state (HttpState).
    pub fn local_state(&self) -> Option<&SQLiteState> {
        match &self.backend {
            StateBackend::Local(state) => Some(state),
            StateBackend::Remote { .. } => None,
        }
    }

    /// Execute an async operation on the state backend.
    fn run_async<F, T>(&self, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        match &self.backend {
            StateBackend::Local(_) => {
                // For local state, we create a minimal runtime just to execute the future.
                // Since SQLiteState's async methods are just sync wrappers, this is fast.
                futures::executor::block_on(f)
            }
            StateBackend::Remote { runtime, .. } => runtime.block_on(f),
        }
    }

    /// Get a reference to the state as a trait object for async operations.
    fn state(&self) -> &dyn StateAccess {
        match &self.backend {
            StateBackend::Local(state) => state,
            StateBackend::Remote { state, .. } => state,
        }
    }

    /// Update worker heartbeat in the database.
    pub fn heartbeat(&mut self) -> WorkerResult<()> {
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.6f")
            .to_string();
        let worker_name = self.config.worker_name.clone();
        let update = WorkerUpdate {
            last_heartbeat: Some(now),
            ..Default::default()
        };
        self.run_async(self.state().update_worker(&worker_name, update))?;
        self.last_heartbeat = Instant::now();
        Ok(())
    }

    /// Check if heartbeat is due.
    pub fn heartbeat_due(&self) -> bool {
        self.last_heartbeat.elapsed() >= Duration::from_secs(self.config.heartbeat_interval)
    }

    /// Set worker status.
    pub fn set_status(&self, status: WorkerStatus) -> WorkerResult<()> {
        let worker_name = self.config.worker_name.clone();
        let update = WorkerUpdate {
            status: Some(status),
            ..Default::default()
        };
        self.run_async(self.state().update_worker(&worker_name, update))?;
        Ok(())
    }

    // =========================================================================
    // Task Operations
    // =========================================================================

    /// List all tasks.
    pub fn task_list(&self) -> WorkerResult<String> {
        let tasks = self.run_async(self.state().get_tasks())?;

        let mut task_outputs = Vec::new();
        for t in &tasks {
            let blocked = self
                .run_async(self.state().is_task_blocked(&t.id))
                .unwrap_or(false);
            task_outputs.push(serde_json::json!({
                "id": t.id,
                "name": t.name,
                "status": t.status.as_str(),
                "claimed_by": t.claimed_by,
                "parent": t.parent_id,
                "blocked_by": t.blocked_by,
                "blocked": blocked,
            }));
        }

        let output = serde_json::json!({ "tasks": task_outputs });
        serde_json::to_string_pretty(&output)
            .map_err(|e| WorkerError::Config(format!("Serialization error: {}", e)))
    }

    /// Claim a task.
    pub fn task_claim(&self, task_id: &str) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        self.run_async(self.state().claim_task(task_id, &worker_name))?;

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
            "claimed_by": self.config.worker_name,
        })
        .to_string())
    }

    /// Mark a task as done.
    pub fn task_done(&self, task_id: Option<&str>) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let tid = match task_id {
            Some(id) => id.to_string(),
            None => {
                // Get currently claimed task
                let task = self
                    .run_async(self.state().get_claimed_task(&worker_name))?
                    .ok_or(WorkerError::NoTaskClaimed)?;
                task.id
            }
        };

        self.run_async(self.state().complete_task(&tid, &worker_name))?;

        // Completing a task might unblock other tasks, so wake awaiting workers
        self.try_resume_awaiting_workers();

        Ok(serde_json::json!({
            "success": true,
            "task_id": tid,
        })
        .to_string())
    }

    /// Unclaim a task.
    pub fn task_unclaim(&self, task_id: Option<&str>) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let tid = match task_id {
            Some(id) => id.to_string(),
            None => {
                let task = self
                    .run_async(self.state().get_claimed_task(&worker_name))?
                    .ok_or(WorkerError::NoTaskClaimed)?;
                task.id
            }
        };

        self.run_async(self.state().unclaim_task(&tid, &worker_name))?;

        // Unclaiming a task makes it available, so wake awaiting workers
        self.try_resume_awaiting_workers();

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

        self.run_async(self.state().add_task(
            task_id,
            name,
            parent,
            if blocked_refs.is_empty() {
                None
            } else {
                Some(blocked_refs.as_slice())
            },
        ))?;

        // New task might be claimable, so wake awaiting workers
        if blocked_refs.is_empty() {
            self.try_resume_awaiting_workers();
        }

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
        })
        .to_string())
    }

    /// Try to resume awaiting workers and scale up if needed.
    /// This provides immediate responsiveness when tasks become available.
    /// The daemon's polling loop also handles this, but with a 5-second interval.
    fn try_resume_awaiting_workers(&self) {
        use crate::core::lifecycle::{LifecycleEvent, LifecycleManager, LocalLifecycleManager};
        use tracing::debug;

        // In remote mode, coordinator handles this
        if std::env::var("HIRSEL_API_URL").is_ok() {
            return;
        }

        // Local mode - use lifecycle manager
        if let Ok(lifecycle) = LocalLifecycleManager::new(
            &self.config.run_name,
            self.config.run_dir.clone(),
            self.config.agent_command.clone(),
        ) {
            // Process TaskCompleted event which handles resume and scale up
            match lifecycle.process_event(LifecycleEvent::TaskCompleted {
                task_id: String::new(),
                worker_name: self.config.worker_name.clone(),
            }) {
                Ok(actions) => {
                    for action in actions {
                        debug!("Lifecycle action: {:?}", action);
                    }
                }
                Err(e) => {
                    debug!("Failed to process lifecycle event: {}", e);
                }
            }
        }
    }

    /// Delete a task.
    pub fn task_delete(&self, task_id: &str) -> WorkerResult<String> {
        self.run_async(self.state().delete_task(task_id))?;

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
        })
        .to_string())
    }

    /// Reopen a completed task.
    pub fn task_undone(&self, task_id: &str) -> WorkerResult<String> {
        self.run_async(self.state().reopen_task(task_id))?;

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
        let claimable = self.run_async(self.state().get_claimable_tasks())?;

        // Note: Lifecycle management (eval triggering, scaling) is handled by
        // the daemon's polling loop, not by individual worker processes

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
        let worker_name = self.config.worker_name.clone();
        self.run_async(self.state().add_message(thread, &worker_name, message))?;

        if wait {
            // Set to Awaiting with hitl_waiting flag
            self.set_status(WorkerStatus::Awaiting)?;
            self.run_async(self.state().update_worker(
                &worker_name,
                WorkerUpdate {
                    hitl_waiting: Some(true),
                    waiting_thread: Some(thread.to_string()),
                    ..Default::default()
                },
            ))?;
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
        let worker_name = self.config.worker_name.clone();
        let messages = if let Some(t) = thread {
            self.run_async(self.state().get_unread_messages(t, &worker_name))?
        } else {
            // Read from all threads
            self.run_async(self.state().get_all_unread_messages(&worker_name))?
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
        let threads = self.run_async(self.state().get_threads())?;

        Ok(serde_json::json!({
            "threads": threads,
        })
        .to_string())
    }

    /// Check inbox for new messages.
    pub fn msg_inbox(&self) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let threads = self.run_async(self.state().get_threads())?;

        let mut inbox = Vec::new();
        for thread in &threads {
            let messages =
                self.run_async(self.state().get_unread_messages(thread, &worker_name))?;

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

    /// Signal that worker has no more work to do.
    /// This sets the worker to Awaiting status and may trigger evaluation
    /// if all workers are inactive.
    pub fn work_done(&self) -> WorkerResult<String> {
        // In remote mode, just set status - coordinator handles lifecycle
        if std::env::var("HIRSEL_API_URL").is_ok() {
            self.set_status(WorkerStatus::Awaiting)?;
        } else {
            // In local mode, use LifecycleManager for immediate response
            use crate::core::lifecycle::{LifecycleManager, LocalLifecycleManager};

            // Set status first
            self.set_status(WorkerStatus::Awaiting)?;

            // Use lifecycle manager to check/trigger eval
            if let Ok(lifecycle) = LocalLifecycleManager::new(
                &self.config.run_name,
                self.config.run_dir.clone(),
                self.config.agent_command.clone(),
            ) {
                let _ = lifecycle.worker_done(&self.config.worker_name);
            }
        }

        Ok(serde_json::json!({
            "success": true,
            "worker": self.config.worker_name,
            "status": "awaiting",
        })
        .to_string())
    }

    // =========================================================================
    // Time Status
    // =========================================================================

    /// Get time information for the run.
    pub fn get_time_info(&self) -> WorkerResult<Option<crate::core::state::TimeInfo>> {
        Ok(self.run_async(self.state().get_time_info())?)
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
        let config = WorkerConfig::new(
            "achilles".into(),
            "test-run".into(),
            PathBuf::from("/tmp/test-run"),
            vec!["hirsel".to_string(), "__acp-bridge".to_string()],
        );
        assert_eq!(config.worker_name, "achilles");
        assert_eq!(config.run_name, "test-run");
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
