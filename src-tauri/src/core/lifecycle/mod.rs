//! Centralized lifecycle management for hirsel runs.
//!
//! This module consolidates all lifecycle logic into a single interface:
//! - Run status transitions (Working → Paused → Working, Working → Eval → Done, etc.)
//! - Worker status management (pause/resume/kill)
//! - Eval triggering when all workers become inactive
//! - Time limit enforcement
//! - Worker scaling
//!
//! The `LifecycleManager` trait provides a single entry point for all lifecycle
//! operations, replacing scattered logic across daemon, coordinator_api,
//! orchestrator, and workers modules.

pub mod local;
pub mod transitions;

pub use local::LocalLifecycleManager;
pub use transitions::{RunStateMachine, WorkerStateMachine};

use crate::core::snapshot::WorkerStateHandle;
use crate::core::state::{FailureReason, Status, WorkerStatus};
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during lifecycle operations.
#[derive(Debug, Error)]
pub enum LifecycleError {
    #[error("State error: {0}")]
    State(String),

    #[error("Invalid transition: {from} -> {to}")]
    InvalidTransition { from: String, to: String },

    #[error("Worker error: {0}")]
    Worker(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Run not active")]
    RunNotActive,

    #[error("Configuration error: {0}")]
    Config(String),
}

pub type LifecycleResult<T> = Result<T, LifecycleError>;

impl From<crate::core::state::StateError> for LifecycleError {
    fn from(e: crate::core::state::StateError) -> Self {
        LifecycleError::State(e.to_string())
    }
}

impl From<crate::core::delta::DeltaStateError> for LifecycleError {
    fn from(e: crate::core::delta::DeltaStateError) -> Self {
        LifecycleError::State(e.to_string())
    }
}

/// Events that trigger lifecycle actions.
#[derive(Debug, Clone)]
pub enum LifecycleEvent {
    /// A worker signaled it has no more work.
    WorkerDone { worker_name: String },

    /// A worker's status changed.
    WorkerStatusChanged {
        worker_name: String,
        old: WorkerStatus,
        new: WorkerStatus,
    },

    /// A task was completed.
    TaskCompleted {
        task_id: String,
        worker_name: String,
    },

    /// A new task was added.
    TaskAdded { task_id: String },

    /// A task was unclaimed (released back to pool).
    TaskUnclaimed { task_id: String },

    /// Periodic time check (from daemon polling).
    TimeCheck,

    /// User requested pause.
    PauseRequested { reason: String },

    /// User requested resume.
    ResumeRequested,

    /// Eval completed (success or failure).
    EvalCompleted { success: bool, feedback: String },
}

/// Actions taken by the lifecycle manager.
#[derive(Debug, Clone)]
pub enum LifecycleAction {
    /// No action was taken.
    None,

    /// Run status changed.
    RunStatusChanged(Status),

    /// Workers were paused.
    WorkersPaused(Vec<String>),

    /// Workers were killed.
    WorkersKilled(Vec<String>),

    /// Workers were resumed.
    WorkersResumed(Vec<String>),

    /// Request to spawn a new worker.
    ///
    /// The caller (daemon) should handle actual spawning via the orchestrator,
    /// which uses the runner system to spawn workers correctly based on runner type.
    SpawnWorker {
        /// Name of the worker to spawn.
        worker_name: String,
        /// Work directory for the worker.
        work_dir: PathBuf,
        /// Task ID to assign to this worker (already claimed in DB).
        assigned_task_id: Option<String>,
    },

    /// Request to resume a paused/awaiting worker.
    ///
    /// Like SpawnWorker, the caller should use the orchestrator to spawn via runner.
    /// The orchestrator handles snapshot restoration for ephemeral runners.
    ResumeWorker {
        /// Name of the worker to resume.
        worker_name: String,
        /// Work directory for the worker.
        work_dir: PathBuf,
        /// Session ID to resume from (if any).
        resume_session_id: Option<String>,
        /// Unified state handle containing work dir snapshot and agent session.
        /// The orchestrator will restore state as needed based on runner type.
        state_handle: Option<WorkerStateHandle>,
    },

    /// Eval was triggered.
    EvalTriggered,

    /// Run completed successfully.
    RunCompleted,

    /// Run failed.
    RunFailed { reason: FailureReason },

    /// Time limit warning sent.
    TimeWarning { percent: i64 },
}

/// Context for lifecycle operations.
#[derive(Debug, Clone)]
pub struct LifecycleContext {
    /// Name of the run.
    pub run_name: String,
    /// Path to the run directory.
    pub run_dir: PathBuf,
    /// Agent command for spawning workers.
    pub agent_command: Vec<String>,
}

impl LifecycleContext {
    pub fn new(run_name: impl Into<String>, run_dir: PathBuf, agent_command: Vec<String>) -> Self {
        Self {
            run_name: run_name.into(),
            run_dir,
            agent_command,
        }
    }
}

/// Centralized lifecycle management trait.
///
/// All lifecycle operations go through this interface. Implementations
/// handle the actual state updates and process management.
///
/// Note: Methods are async since SQLite operations are now async with sqlx.
#[allow(async_fn_in_trait)]
pub trait LifecycleManager {
    /// Process a lifecycle event and return actions taken.
    ///
    /// This is the main entry point for all lifecycle operations.
    /// Events trigger state checks and appropriate actions.
    async fn process_event(&self, event: LifecycleEvent) -> LifecycleResult<Vec<LifecycleAction>>;

    /// Pause the run, killing all active workers.
    ///
    /// Workers will be marked as Paused and can be resumed later.
    async fn pause_run(&self, reason: &str) -> LifecycleResult<Vec<String>>;

    /// Resume the run, respawning paused workers.
    ///
    /// Returns a list of ResumeWorker actions for the daemon to process.
    async fn resume_run(&self) -> LifecycleResult<Vec<LifecycleAction>>;

    /// Handle a worker signaling it's done with work.
    ///
    /// This may trigger eval or mark the run as done if all workers are inactive.
    async fn worker_done(&self, worker_name: &str) -> LifecycleResult<Vec<LifecycleAction>>;

    /// Handle time limit expiration.
    ///
    /// Kills all workers and sets run to Failed with TimeLimit reason.
    async fn handle_time_expired(&self) -> LifecycleResult<()>;

    /// Check if all workers are inactive (awaiting or error).
    async fn all_workers_inactive(&self) -> LifecycleResult<bool>;

    /// Check if eval should be triggered.
    ///
    /// Returns true if all workers are inactive and run is in Working status.
    async fn should_trigger_eval(&self) -> LifecycleResult<bool>;

    /// Check if worker scaling is possible.
    async fn can_scale_up(&self) -> LifecycleResult<bool>;

    /// Get current run status.
    async fn run_status(&self) -> LifecycleResult<Status>;

    /// Get the lifecycle context.
    fn context(&self) -> &LifecycleContext;
}
