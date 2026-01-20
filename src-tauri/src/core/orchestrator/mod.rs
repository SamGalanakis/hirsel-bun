//! Orchestrator abstraction for remote/local Hirsel coordination
//!
//! This module provides the `Orchestrator` trait that abstracts high-level
//! operations for managing runs, workers, tasks, and messages. It has two
//! implementations:
//! - `LocalOrchestrator`: Direct calls to local state (default)
//! - `RemoteOrchestrator`: HTTP calls to a remote Hirsel server

#[cfg(feature = "server")]
mod daemon;
mod local;
mod remote;

#[cfg(feature = "server")]
pub use daemon::DaemonOrchestrator;
pub use local::LocalOrchestrator;
pub use remote::RemoteOrchestrator;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::api_types::{
    ConfigResponse, Eval, HistoryEntry, Message, RunDetail, RunSummary, Task, ThreadSummary,
    Worker, WorkerEventsResponse,
};
use crate::core::config::{self, Config};

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
