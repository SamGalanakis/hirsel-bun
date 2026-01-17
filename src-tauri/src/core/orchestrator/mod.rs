//! Orchestrator abstraction for remote/local Hirsel coordination
//!
//! This module provides the `Orchestrator` trait that abstracts high-level
//! operations for managing runs, workers, tasks, and messages. It has two
//! implementations:
//! - `LocalOrchestrator`: Direct calls to local state (default)
//! - `RemoteOrchestrator`: HTTP calls to a remote Hirsel server

mod local;
mod remote;

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

/// Worker log request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerLogParams {
    pub lines: Option<usize>,
}

/// Server health response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
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

    /// Get worker log content (deprecated: use get_worker_events instead)
    async fn get_worker_log(
        &self,
        run: &str,
        worker: &str,
        lines: Option<usize>,
    ) -> OrchestratorResult<String>;

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
}

// =============================================================================
// Factory Function
// =============================================================================

/// Create an orchestrator based on the profile configuration
///
/// If no profile is specified, uses the default profile from config.
/// Returns a LocalOrchestrator for local mode, RemoteOrchestrator for remote.
pub fn create_orchestrator(profile: Option<&str>) -> OrchestratorResult<Box<dyn Orchestrator>> {
    use crate::core::credentials::CredentialStore;

    let (config, _) = Config::load().map_err(|e| OrchestratorError::Config(e.to_string()))?;

    let profile_name = profile.unwrap_or(&config.default_profile);

    let profile_config = config
        .profiles
        .get(profile_name)
        .ok_or_else(|| OrchestratorError::UnknownProfile(profile_name.to_string()))?;

    match profile_config.mode {
        config::OrchestratorMode::Local => Ok(Box::new(LocalOrchestrator::new(config))),
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

/// Create a local orchestrator directly (bypasses profile resolution)
pub fn create_local_orchestrator() -> OrchestratorResult<LocalOrchestrator> {
    let (config, _) = Config::load().map_err(|e| OrchestratorError::Config(e.to_string()))?;
    Ok(LocalOrchestrator::new(config))
}
