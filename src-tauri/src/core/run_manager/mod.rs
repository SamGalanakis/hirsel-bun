//! Unified run management trait.
//!
//! This module combines the functionality of the previous `Orchestrator` and
//! `LifecycleManager` traits into a single unified `RunManager` trait.
//!
//! ## Current Status
//!
//! This is a thin wrapper around the existing Orchestrator trait for now.
//! The implementations delegate to LocalOrchestrator and RemoteOrchestrator
//! respectively. The key addition is the `poll_lifecycle` method which
//! the daemon can call to process lifecycle events.
//!
//! Future phases will consolidate more logic into RunManager and simplify
//! the orchestrator/lifecycle split.

mod local;
mod remote;

pub use local::LocalRunManager;
pub use remote::RemoteRunManager;

use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;

use crate::core::api_types::{
    ConfigResponse, Eval, HistoryEntry, RunDetail, RunSummary, Worker, WorkerEventsResponse,
};
use crate::core::config::Config;
use crate::core::orchestrator::HealthResponse;

// =============================================================================
// Error Types
// =============================================================================

#[derive(Debug, Error)]
pub enum RunManagerError {
    #[error("Run not found: {0}")]
    RunNotFound(String),

    #[error("Worker not found: {0}")]
    WorkerNotFound(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("State error: {0}")]
    State(String),

    #[error("Runner error: {0}")]
    Runner(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Other error: {0}")]
    Other(String),
}

pub type RunManagerResult<T> = Result<T, RunManagerError>;

impl From<crate::core::orchestrator::OrchestratorError> for RunManagerError {
    fn from(e: crate::core::orchestrator::OrchestratorError) -> Self {
        RunManagerError::Other(e.to_string())
    }
}

// =============================================================================
// RunManager Trait
// =============================================================================

/// Unified run management trait.
///
/// This trait wraps the Orchestrator interface and adds lifecycle polling.
/// For now it delegates most operations to the underlying orchestrator.
#[async_trait]
pub trait RunManager: Send + Sync {
    // =========================================================================
    // Run CRUD
    // =========================================================================

    async fn list_runs(&self) -> RunManagerResult<Vec<RunSummary>>;
    async fn get_run(&self, name: &str) -> RunManagerResult<RunDetail>;
    async fn delete_run(&self, name: &str) -> RunManagerResult<()>;

    // =========================================================================
    // Run Lifecycle
    // =========================================================================

    async fn pause_run(&self, name: &str) -> RunManagerResult<()>;
    async fn resume_run(&self, name: &str, time_limit_minutes: Option<u32>)
        -> RunManagerResult<()>;
    async fn deliver_run(&self, name: &str, branch: Option<String>) -> RunManagerResult<String>;

    // =========================================================================
    // Workers
    // =========================================================================

    async fn list_workers(&self, run: &str) -> RunManagerResult<Vec<Worker>>;
    async fn restart_worker(&self, run: &str, worker: &str) -> RunManagerResult<()>;
    async fn get_worker_events(
        &self,
        run: &str,
        worker: &str,
        after_id: Option<i64>,
        limit: Option<i64>,
    ) -> RunManagerResult<WorkerEventsResponse>;

    // =========================================================================
    // Evals & History
    // =========================================================================

    async fn list_evals(&self, run: &str) -> RunManagerResult<Vec<Eval>>;
    async fn get_history(
        &self,
        run: &str,
        limit: Option<u32>,
    ) -> RunManagerResult<Vec<HistoryEntry>>;

    // =========================================================================
    // Lifecycle Polling (for daemon)
    // =========================================================================

    /// Poll a run's lifecycle and execute any necessary actions.
    ///
    /// This is called periodically by the daemon for each active run.
    /// It checks run state, handles worker completion, triggers evals, etc.
    ///
    /// For `LocalRunManager`, this processes the lifecycle state machine
    /// and spawns/stops workers as needed.
    ///
    /// For `RemoteRunManager`, this is a no-op (coordinator handles it).
    async fn poll_lifecycle(&self, run: &str) -> RunManagerResult<()>;

    // =========================================================================
    // Config & Health
    // =========================================================================

    async fn get_config(&self) -> RunManagerResult<ConfigResponse>;
    async fn health(&self) -> RunManagerResult<HealthResponse>;
}

// =============================================================================
// Factory Functions
// =============================================================================

/// Create a run manager based on configuration.
pub fn create_run_manager() -> RunManagerResult<Arc<dyn RunManager>> {
    let (config, _) = Config::load().map_err(|e| RunManagerError::Other(e.to_string()))?;

    if let Some(ref url) = config.backend.url {
        let api_key = config.backend.api_key.clone().unwrap_or_default();
        Ok(Arc::new(RemoteRunManager::new(url.clone(), api_key)))
    } else {
        Ok(Arc::new(LocalRunManager::new(config)))
    }
}

/// Create a local run manager directly.
pub fn create_local_run_manager() -> RunManagerResult<LocalRunManager> {
    let (config, _) = Config::load().map_err(|e| RunManagerError::Other(e.to_string()))?;
    Ok(LocalRunManager::new(config))
}
