//! Remote run manager implementation.
//!
//! Wraps the existing RemoteOrchestrator to provide the unified RunManager interface.
//! All operations are delegated to a remote Hirsel server via HTTP.

use async_trait::async_trait;

use super::{RunManager, RunManagerError, RunManagerResult};
use crate::core::api_types::{
    ConfigResponse, Eval, HistoryEntry, RunDetail, RunSummary, Worker, WorkerEventsResponse,
};
use crate::core::orchestrator::{HealthResponse, Orchestrator, RemoteOrchestrator};

/// Remote run manager that communicates with a Hirsel server over HTTP.
pub struct RemoteRunManager {
    orchestrator: RemoteOrchestrator,
}

impl RemoteRunManager {
    /// Create a new remote run manager with the given server URL and API key.
    pub fn new(base_url: String, api_key: String) -> Self {
        let orchestrator = RemoteOrchestrator::new(base_url, api_key);
        Self { orchestrator }
    }
}

#[async_trait]
impl RunManager for RemoteRunManager {
    // =========================================================================
    // Run CRUD
    // =========================================================================

    async fn list_runs(&self) -> RunManagerResult<Vec<RunSummary>> {
        self.orchestrator
            .list_runs()
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }

    async fn get_run(&self, name: &str) -> RunManagerResult<RunDetail> {
        self.orchestrator
            .get_run(name)
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }

    async fn delete_run(&self, name: &str) -> RunManagerResult<()> {
        self.orchestrator
            .delete_run(name)
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }

    // =========================================================================
    // Run Lifecycle
    // =========================================================================

    async fn pause_run(&self, name: &str) -> RunManagerResult<()> {
        self.orchestrator
            .pause_run(name)
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }

    async fn resume_run(
        &self,
        name: &str,
        time_limit_minutes: Option<u32>,
    ) -> RunManagerResult<()> {
        self.orchestrator
            .resume_run(name, time_limit_minutes)
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }

    async fn deliver_run(&self, name: &str, branch: Option<String>) -> RunManagerResult<String> {
        self.orchestrator
            .deliver_run(name, branch)
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }

    // =========================================================================
    // Workers
    // =========================================================================

    async fn list_workers(&self, run: &str) -> RunManagerResult<Vec<Worker>> {
        self.orchestrator
            .list_workers(run)
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }

    async fn restart_worker(&self, run: &str, worker: &str) -> RunManagerResult<()> {
        self.orchestrator
            .restart_worker(run, worker)
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }

    async fn get_worker_events(
        &self,
        run: &str,
        worker: &str,
        after_id: Option<i64>,
        limit: Option<i64>,
    ) -> RunManagerResult<WorkerEventsResponse> {
        self.orchestrator
            .get_worker_events(run, worker, after_id, limit)
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }

    // =========================================================================
    // Evals & History
    // =========================================================================

    async fn list_evals(&self, run: &str) -> RunManagerResult<Vec<Eval>> {
        self.orchestrator
            .list_evals(run)
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }

    async fn get_history(
        &self,
        run: &str,
        limit: Option<u32>,
    ) -> RunManagerResult<Vec<HistoryEntry>> {
        self.orchestrator
            .get_history(run, limit)
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }

    // =========================================================================
    // Lifecycle Polling - no-op for remote (coordinator handles it)
    // =========================================================================

    async fn poll_lifecycle(&self, _run: &str) -> RunManagerResult<()> {
        // For remote mode, the coordinator handles lifecycle polling.
        // This is a no-op on the client side.
        Ok(())
    }

    // =========================================================================
    // Config & Health
    // =========================================================================

    async fn get_config(&self) -> RunManagerResult<ConfigResponse> {
        self.orchestrator
            .get_config()
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }

    async fn health(&self) -> RunManagerResult<HealthResponse> {
        self.orchestrator
            .health()
            .await
            .map_err(|e| RunManagerError::Http(e.to_string()))
    }
}
