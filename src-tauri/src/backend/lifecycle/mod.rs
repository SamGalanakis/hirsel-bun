//! Centralized lifecycle management for Hirsel runtimes.

pub mod local;
pub mod transitions;

pub use local::LocalLifecycleManager;
pub use transitions::RunStateMachine;

use crate::backend::snapshot::WorkerStateHandle;
use crate::backend::state::{FailureReason, Status, WorkerStatus};
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

impl From<crate::backend::state::StateError> for LifecycleError {
    fn from(e: crate::backend::state::StateError) -> Self {
        LifecycleError::State(e.to_string())
    }
}

impl From<crate::backend::delta::DeltaStateError> for LifecycleError {
    fn from(e: crate::backend::delta::DeltaStateError) -> Self {
        LifecycleError::State(e.to_string())
    }
}

/// Events that trigger lifecycle actions.
#[derive(Debug, Clone)]
pub enum LifecycleEvent {
    /// A worker's status changed.
    WorkerStatusChanged {
        worker_name: String,
        old: WorkerStatus,
        new: WorkerStatus,
    },

    /// Periodic time check (from daemon polling).
    TimeCheck,
}

/// Actions taken by the lifecycle manager.
#[derive(Debug, Clone)]
pub enum LifecycleAction {
    /// No action was taken.
    None,

    /// Runtime status changed.
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

    /// Runtime completed successfully.
    RunCompleted,

    /// Runtime failed.
    RunFailed { reason: FailureReason },

    /// Time limit warning sent.
    TimeWarning { percent: i64 },
}

/// Context for lifecycle operations.
#[derive(Debug, Clone)]
pub struct LifecycleContext {
    /// Name of the runtime.
    pub runtime_name: String,
    /// Path to the runtime directory.
    pub runtime_dir: PathBuf,
}

impl LifecycleContext {
    pub fn new(
        runtime_name: impl Into<String>,
        runtime_dir: PathBuf,
        _agent_command: Vec<String>,
    ) -> Self {
        Self {
            runtime_name: runtime_name.into(),
            runtime_dir,
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

    /// Pause the runtime, killing all active workers.
    ///
    /// Workers will be marked as Paused and can be resumed later.
    async fn pause_run(&self, reason: &str) -> LifecycleResult<Vec<String>>;

    /// Resume the runtime, respawning paused workers.
    ///
    /// Returns a list of ResumeWorker actions for the daemon to process.
    async fn resume_run(&self) -> LifecycleResult<Vec<LifecycleAction>>;

    /// Handle time limit expiration.
    ///
    /// Kills all workers and sets the runtime to Failed with TimeLimit reason.
    async fn handle_time_expired(&self) -> LifecycleResult<()>;

    /// Get current run status.
    async fn run_status(&self) -> LifecycleResult<Status>;
}
