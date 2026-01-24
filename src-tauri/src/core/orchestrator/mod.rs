//! Coordinator-facing run management for creating runs, spawning workers, and lifecycle.
//!
//! This module provides the `Orchestrator` trait that abstracts high-level
//! operations for managing runs across the entire system. The CLI and GUI use
//! this trait to create runs, spawn workers, and control run lifecycle without
//! knowing if they're operating locally or against a remote server.
//!
//! ## Implementations
//!
//! - `LocalOrchestrator`: Direct calls to local state (default for CLI/GUI)
//! - `RemoteOrchestrator`: HTTP calls to a remote Hirsel server
//! - `DaemonOrchestrator`: Server-side implementation for handling remote requests
//!
//! ## Orchestrator vs StateAccess
//!
//! These two traits serve different purposes:
//!
//! - **`Orchestrator`** (this module): Coordinator-side, cross-run management
//!   - Creating and deleting runs
//!   - Spawning workers
//!   - Managing run lifecycle (pause, resume, deliver)
//!   - Listing runs and their status
//!
//! - **`StateAccess`** (see `state_access` module): Worker-side, per-run operations
//!   - Task claiming and completion
//!   - Worker heartbeats and status updates
//!   - Message sending between workers
//!   - Reading/writing run configuration
//!
//! The CLI/GUI uses `Box<dyn Orchestrator>` for run management commands.
//! Workers receive a `Box<dyn StateAccess>` for runtime state operations.

#[cfg(feature = "server")]
mod daemon;
mod local;
mod remote;
#[cfg(test)]
pub mod test_harness;

#[cfg(feature = "server")]
pub use daemon::DaemonOrchestrator;
pub use local::LocalOrchestrator;
pub use remote::RemoteOrchestrator;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use std::collections::HashMap;

use crate::core::api_types::{
    ConfigResponse, Eval, HistoryEntry, Message, RunDetail, RunSummary, Task, ThreadSummary,
    Worker, WorkerEventsResponse,
};
use crate::core::config::{self, Config};
use crate::core::draft::StartingPoint;
use crate::core::snapshot::WorkerStateHandle;

// =============================================================================
// Error Types
// =============================================================================

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("Run not found: {0}")]
    RunNotFound(String),

    #[error("Worker not found: {0}")]
    WorkerNotFound(String),

    #[error("Task not found: {0}")]
    TaskNotFound(String),

    #[error("State error: {0}")]
    State(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Unknown profile: {0}")]
    UnknownProfile(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("{0}")]
    Other(String),
}

impl From<reqwest::Error> for OrchestratorError {
    fn from(e: reqwest::Error) -> Self {
        OrchestratorError::Http(e.to_string())
    }
}

impl From<crate::core::state::StateError> for OrchestratorError {
    fn from(e: crate::core::state::StateError) -> Self {
        OrchestratorError::State(e.to_string())
    }
}

pub type OrchestratorResult<T> = Result<T, OrchestratorError>;

// =============================================================================
// DTOs for API communication
// =============================================================================

/// Resume run request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeRunRequest {
    pub time_limit_minutes: Option<u32>,
}

/// Deliver run request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliverRunRequest {
    pub branch: Option<String>,
}

/// Add task request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddTaskRequest {
    pub content: String,
}

/// Send message request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub content: String,
}

/// Server health response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// Tailscale OAuth credentials for generating ephemeral auth keys
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscaleOAuth {
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub tag: Option<String>,
}

/// Create run request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRunRequest {
    /// Name of the run
    pub name: String,
    /// Spec content (markdown)
    pub spec: String,
    /// Starting point for workspace (how to initialize the work directory)
    /// If None, expects files to be uploaded via upload_files()
    pub starting_point: Option<StartingPoint>,
    /// Optional runner name (default: from server config)
    pub runner: Option<String>,
    /// Maximum workers to autoscale to (default: 1)
    pub worker_scale: Option<u32>,
    /// Time limit in minutes
    pub time_limit_minutes: Option<u32>,
    /// Max iterations before pausing
    pub max_iterations: Option<u32>,
    /// Human-in-the-loop mode
    pub human_in_the_loop: Option<bool>,
    /// Eval file content (markdown)
    pub eval: Option<String>,
    /// Tailscale OAuth credentials for worker hosts to join tailnet
    pub tailscale_oauth: Option<TailscaleOAuth>,
}

/// Create run response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRunResponse {
    pub name: String,
    pub run_dir: String,
    pub files_url: String,
}

/// Spawn workers request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnWorkersRequest {
    /// Number of workers to spawn
    pub count: u32,
}

/// Spawn workers response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnWorkersResponse {
    /// Names of spawned workers
    pub workers: Vec<String>,
}

/// Request to spawn a single worker (used by daemon for lifecycle management)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnSingleWorkerRequest {
    /// Work directory path for the worker
    pub work_dir: String,
    /// Optional session ID to resume from
    pub resume_session_id: Option<String>,
}

/// Request to resume a worker (handles snapshot/session restoration)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeWorkerRequest {
    /// Work directory path for the worker
    pub work_dir: String,
    /// Optional session ID to resume from
    pub resume_session_id: Option<String>,
    /// Unified state handle containing work dir snapshot and agent session
    pub state_handle: Option<crate::core::snapshot::WorkerStateHandle>,
}

/// Request to initialize or reinitialize workspace for a run
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitWorkspaceRequest {
    /// Starting point for workspace initialization
    pub starting_point: StartingPoint,
}

/// Response from workspace initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitWorkspaceResponse {
    /// Path to the workspace
    pub workspace_path: String,
    /// Default git branch (if applicable)
    pub default_branch: Option<String>,
}

/// Request to start a run (unified entry point for CLI and GUI)
///
/// This combines workspace setup and worker spawning into a single operation.
/// The orchestrator handles:
/// 1. Workspace creation based on starting_point
/// 2. Worker registration in state
/// 3. Worker spawning via the appropriate Runner (unless draft mode)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRunRequest {
    /// Run name (will be slugified)
    pub name: String,
    /// Spec content (markdown)
    pub spec: String,
    /// Starting point for workspace (how to initialize the work directory)
    pub starting_point: StartingPoint,
    /// Optional eval content (markdown)
    pub eval: Option<String>,
    /// Worker scale (max workers for autoscaling, default: 1)
    pub worker_scale: Option<u32>,
    /// Time limit in minutes
    pub time_limit_minutes: Option<i64>,
    /// Max iterations before pausing
    pub max_iterations: Option<i64>,
    /// Human-in-the-loop mode (default: true)
    pub human_in_the_loop: Option<bool>,
    /// Runner name (default: from config or "local")
    pub runner: Option<String>,
    /// Per-worker runner assignments
    pub worker_runners: Option<HashMap<String, String>>,
    /// Tailscale OAuth credentials for worker hosts to join tailnet
    pub tailscale_oauth: Option<TailscaleOAuth>,
    /// Draft mode - setup run but don't spawn workers (default: false)
    #[serde(default)]
    pub draft: bool,
}

// =============================================================================
// Orchestrator Trait
// =============================================================================

/// High-level orchestrator trait for managing Hirsel runs
///
/// This trait abstracts the coordination layer, allowing both local and remote
/// implementations. The CLI and GUI use this trait to perform all operations.
#[async_trait]
pub trait Orchestrator: Send + Sync {
    // -------------------------------------------------------------------------
    // Run Management
    // -------------------------------------------------------------------------

    /// List all runs
    async fn list_runs(&self) -> OrchestratorResult<Vec<RunSummary>>;

    /// Get detailed information about a specific run
    async fn get_run(&self, name: &str) -> OrchestratorResult<RunDetail>;

    /// Delete a run
    async fn delete_run(&self, name: &str) -> OrchestratorResult<()>;

    /// Pause a running run
    async fn pause_run(&self, name: &str) -> OrchestratorResult<()>;

    /// Resume a paused run
    async fn resume_run(
        &self,
        name: &str,
        time_limit_minutes: Option<u32>,
    ) -> OrchestratorResult<()>;

    /// Deliver run changes to a branch
    async fn deliver_run(&self, name: &str, branch: Option<String>) -> OrchestratorResult<String>;

    // -------------------------------------------------------------------------
    // Workers
    // -------------------------------------------------------------------------

    /// List workers for a run
    async fn list_workers(&self, run: &str) -> OrchestratorResult<Vec<Worker>>;

    /// Restart a specific worker
    async fn restart_worker(&self, run: &str, worker: &str) -> OrchestratorResult<()>;

    /// Get worker events for streaming from database
    async fn get_worker_events(
        &self,
        run: &str,
        worker: &str,
        after_id: Option<i64>,
        limit: Option<i64>,
    ) -> OrchestratorResult<WorkerEventsResponse>;

    // -------------------------------------------------------------------------
    // Tasks
    // -------------------------------------------------------------------------

    /// List tasks for a run
    async fn list_tasks(&self, run: &str) -> OrchestratorResult<Vec<Task>>;

    /// Add a new task
    async fn add_task(&self, run: &str, content: &str) -> OrchestratorResult<Task>;

    /// Delete a task
    async fn delete_task(&self, run: &str, task_id: &str) -> OrchestratorResult<()>;

    /// Mark a task as complete
    async fn complete_task(&self, run: &str, task_id: &str) -> OrchestratorResult<()>;

    /// Reopen a completed task
    async fn reopen_task(&self, run: &str, task_id: &str) -> OrchestratorResult<()>;

    // -------------------------------------------------------------------------
    // Messages
    // -------------------------------------------------------------------------

    /// List message threads for a run
    async fn list_threads(&self, run: &str) -> OrchestratorResult<Vec<ThreadSummary>>;

    /// Get messages in a thread
    async fn get_messages(&self, run: &str, thread: &str) -> OrchestratorResult<Vec<Message>>;

    /// Send a message to a thread
    async fn send_message(
        &self,
        run: &str,
        thread: &str,
        content: &str,
    ) -> OrchestratorResult<Message>;

    // -------------------------------------------------------------------------
    // Evals
    // -------------------------------------------------------------------------

    /// List evaluations for a run
    async fn list_evals(&self, run: &str) -> OrchestratorResult<Vec<Eval>>;

    // -------------------------------------------------------------------------
    // History
    // -------------------------------------------------------------------------

    /// Get run history/activity log
    async fn get_history(
        &self,
        run: &str,
        limit: Option<u32>,
    ) -> OrchestratorResult<Vec<HistoryEntry>>;

    // -------------------------------------------------------------------------
    // Configuration
    // -------------------------------------------------------------------------

    /// Get server/client configuration
    async fn get_config(&self) -> OrchestratorResult<ConfigResponse>;

    /// Check server health (for remote orchestrator)
    async fn health(&self) -> OrchestratorResult<HealthResponse>;

    // -------------------------------------------------------------------------
    // Run Creation (for CLI/GUI use)
    // -------------------------------------------------------------------------

    /// Create a new run (sets up directories and state, doesn't spawn workers)
    ///
    /// This method creates the run directory, initializes the database,
    /// writes the spec file, and registers the initial worker. After this,
    /// call `upload_files()` to provide project files, then `spawn_workers()`.
    async fn create_run(&self, request: CreateRunRequest) -> OrchestratorResult<CreateRunResponse> {
        let _ = request;
        Err(OrchestratorError::Other(
            "create_run not implemented for this orchestrator".to_string(),
        ))
    }

    /// Upload project files for a run (tarball)
    ///
    /// For LocalOrchestrator: Extracts tarball to work/ directory
    /// For RemoteOrchestrator: HTTP POST to server
    async fn upload_files(&self, run_name: &str, tarball: Vec<u8>) -> OrchestratorResult<()> {
        let _ = (run_name, tarball);
        Err(OrchestratorError::Other(
            "upload_files not implemented for this orchestrator".to_string(),
        ))
    }

    /// Spawn workers for a run
    ///
    /// Creates and starts the specified number of workers.
    /// The run must have files uploaded first (for remote workers).
    async fn spawn_workers(
        &self,
        run_name: &str,
        count: u32,
    ) -> OrchestratorResult<SpawnWorkersResponse> {
        let _ = (run_name, count);
        Err(OrchestratorError::Other(
            "spawn_workers not implemented for this orchestrator".to_string(),
        ))
    }

    /// Start a run (unified entry point for CLI and GUI)
    ///
    /// This is the preferred method for starting runs as it combines:
    /// 1. Run directory and database setup
    /// 2. Workspace creation from the starting point
    /// 3. Worker registration and spawning
    ///
    /// For LocalOrchestrator: Handles all operations locally
    /// For RemoteOrchestrator: Delegates to server via HTTP
    ///
    /// Note: The existing `create_run` + `upload_files` + `spawn_workers` flow
    /// is kept for backward compatibility with remote workers that need
    /// fine-grained control over the process.
    async fn start_run(&self, request: StartRunRequest) -> OrchestratorResult<RunDetail> {
        let _ = request;
        Err(OrchestratorError::Other(
            "start_run not implemented for this orchestrator".to_string(),
        ))
    }

    /// Initialize or reinitialize workspace for a run
    ///
    /// This method allows workspace setup to be done separately from run creation.
    /// It can be used to:
    /// 1. Initialize workspace for a run created without a starting_point
    /// 2. Reinitialize workspace (e.g., to switch to a different branch)
    ///
    /// The run must already exist and not have active workers.
    async fn init_workspace(
        &self,
        run_name: &str,
        request: InitWorkspaceRequest,
    ) -> OrchestratorResult<InitWorkspaceResponse> {
        let _ = (run_name, request);
        Err(OrchestratorError::Other(
            "init_workspace not implemented for this orchestrator".to_string(),
        ))
    }

    /// Spawn a single worker for an existing run.
    ///
    /// This is used by the daemon to spawn additional workers during autoscaling
    /// or to resume paused/awaiting workers. Unlike `spawn_workers` which is for
    /// initial run creation, this handles spawning in the context of an already
    /// running run.
    ///
    /// The runner system is used to ensure workers spawn correctly based on
    /// the runner configuration (local, docker, fly, ssh, etc.).
    async fn spawn_single_worker(
        &self,
        run_name: &str,
        worker_name: &str,
        work_dir: &std::path::Path,
        resume_session_id: Option<&str>,
    ) -> OrchestratorResult<()>;

    /// Resume a paused/stopped worker.
    ///
    /// This handles the full resume flow:
    /// 1. Checks if worker is already running (skips if yes)
    /// 2. Restores snapshot if runner is ephemeral and snapshot exists
    /// 3. Restores agent session if handle exists
    /// 4. Spawns worker via runner
    /// 5. Updates worker state (pid, runner_id, status)
    ///
    /// Unlike `spawn_single_worker`, this method handles snapshot restoration
    /// for ephemeral runners (Fly) and checks if the worker is already
    /// running to avoid duplicate container errors.
    async fn resume_worker(
        &self,
        run_name: &str,
        worker_name: &str,
        work_dir: &std::path::Path,
        resume_session_id: Option<&str>,
        state_handle: Option<&WorkerStateHandle>,
    ) -> OrchestratorResult<()>;
}

// =============================================================================
// Factory Function
// =============================================================================

/// Create an orchestrator based on the profile configuration
///
/// If no profile is specified, uses the default profile from config.
/// For local mode, returns a LocalOrchestrator (direct access to local state).
/// For remote mode, connects to the remote server.
///
/// Note: For local mode with daemon lifecycle management, use `create_daemon_orchestrator()`.
pub fn create_orchestrator(profile: Option<&str>) -> OrchestratorResult<Box<dyn Orchestrator>> {
    use crate::core::credentials::CredentialStore;

    let (config, _) = Config::load().map_err(|e| OrchestratorError::Config(e.to_string()))?;

    let profile_name = profile.unwrap_or(&config.default_profile);

    let profile_config = config
        .profiles
        .get(profile_name)
        .ok_or_else(|| OrchestratorError::UnknownProfile(profile_name.to_string()))?;

    match profile_config.mode {
        config::OrchestratorMode::Local => {
            // For local mode, use LocalOrchestrator for direct access
            // The daemon runs separately and handles lifecycle management
            Ok(Box::new(LocalOrchestrator::new(config)))
        }
        config::OrchestratorMode::Remote => {
            let url = profile_config.url.as_ref().ok_or_else(|| {
                OrchestratorError::Config("Missing URL for remote profile".into())
            })?;

            // Try to load API key from credential store first, fall back to config
            let key = {
                let cred_key = format!("profile_{}_api_key", profile_name);
                CredentialStore::open()
                    .ok()
                    .and_then(|store| store.load(&cred_key).ok())
                    .or_else(|| profile_config.api_key.clone())
            }
            .ok_or_else(|| {
                OrchestratorError::Config("Missing API key for remote profile".into())
            })?;

            Ok(Box::new(RemoteOrchestrator::new(url.clone(), key)))
        }
    }
}

/// Create a daemon orchestrator that communicates with the local daemon
///
/// This starts the daemon if it's not running and returns an orchestrator
/// that communicates via Unix socket. Use this when you want the daemon
/// to handle operations (e.g., for CLI commands that should trigger
/// daemon lifecycle management).
#[cfg(feature = "server")]
pub fn create_daemon_orchestrator() -> OrchestratorResult<DaemonOrchestrator> {
    DaemonOrchestrator::connect_or_start()
}

/// Create a local orchestrator directly (bypasses profile resolution)
pub fn create_local_orchestrator() -> OrchestratorResult<LocalOrchestrator> {
    let (config, _) = Config::load().map_err(|e| OrchestratorError::Config(e.to_string()))?;
    Ok(LocalOrchestrator::new(config))
}
