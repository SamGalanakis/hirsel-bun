//! DaemonOrchestrator - Orchestrator that communicates with the local daemon
//!
//! This orchestrator sends all operations to the local hirsel daemon via Unix socket.
//! The daemon handles worker lifecycle, eval triggering, and other background operations.

use async_trait::async_trait;

use super::{
    DeliverRunRequest, HealthResponse, Orchestrator, OrchestratorError, OrchestratorResult,
    ResumeRunRequest, ResumeWorkerRequest, SpawnSingleWorkerRequest, StartRunRequest,
};
use crate::backend::api_types::{
    ConfigResponse, Eval, HistoryEntry, RunDetail, RunSummary, Worker, WorkerEventsResponse,
};
use crate::backend::daemon::DaemonClient;
use crate::backend::snapshot::WorkerStateHandle;

/// Orchestrator that communicates with the local daemon
pub struct DaemonOrchestrator {
    client: DaemonClient,
}

impl DaemonOrchestrator {
    /// Create a new DaemonOrchestrator by connecting to the daemon
    pub fn connect() -> OrchestratorResult<Self> {
        let client = DaemonClient::connect()
            .map_err(|e| OrchestratorError::Other(format!("Failed to connect to daemon: {}", e)))?;
        Ok(Self { client })
    }

    /// Create a new DaemonOrchestrator, starting the daemon if needed
    pub fn connect_or_start() -> OrchestratorResult<Self> {
        let client = DaemonClient::connect_or_start()
            .map_err(|e| OrchestratorError::Other(format!("Failed to connect to daemon: {}", e)))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl Orchestrator for DaemonOrchestrator {
    // -------------------------------------------------------------------------
    // Run Management
    // -------------------------------------------------------------------------

    async fn list_runs(&self) -> OrchestratorResult<Vec<RunSummary>> {
        self.client
            .get("/api/runtimes")
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn get_run(&self, name: &str) -> OrchestratorResult<RunDetail> {
        self.client
            .get(&format!("/api/runtimes/{}", name))
            .await
            .map_err(|e| {
                if e.to_string().contains("404") {
                    OrchestratorError::RunNotFound(name.to_string())
                } else {
                    OrchestratorError::Other(e.to_string())
                }
            })
    }

    async fn delete_run(&self, name: &str) -> OrchestratorResult<()> {
        let _: serde_json::Value = self
            .client
            .delete(&format!("/api/runtimes/{}", name))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn pause_run(&self, name: &str) -> OrchestratorResult<()> {
        let _: serde_json::Value = self
            .client
            .post_empty(&format!("/api/runtimes/{}/pause", name))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn resume_run(
        &self,
        name: &str,
        time_limit_minutes: Option<u32>,
    ) -> OrchestratorResult<()> {
        let request = ResumeRunRequest { time_limit_minutes };
        let _: serde_json::Value = self
            .client
            .post(&format!("/api/runtimes/{}/resume", name), request)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn deliver_run(&self, name: &str, branch: Option<String>) -> OrchestratorResult<String> {
        #[derive(serde::Deserialize)]
        struct DeliverResponse {
            branch: String,
        }

        let request = DeliverRunRequest { branch };
        let response: DeliverResponse = self
            .client
            .post(&format!("/api/runtimes/{}/deliver", name), request)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(response.branch)
    }

    // -------------------------------------------------------------------------
    // Workers
    // -------------------------------------------------------------------------

    async fn list_workers(&self, run: &str) -> OrchestratorResult<Vec<Worker>> {
        self.client
            .get(&format!("/api/runtimes/{}/workers", run))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn restart_worker(&self, run: &str, worker: &str) -> OrchestratorResult<()> {
        let _: serde_json::Value = self
            .client
            .post_empty(&format!("/api/runtimes/{}/workers/{}/restart", run, worker))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn get_worker_events(
        &self,
        run: &str,
        worker: &str,
        after_id: Option<i64>,
        limit: Option<i64>,
    ) -> OrchestratorResult<WorkerEventsResponse> {
        let mut path = format!("/api/runtimes/{}/workers/{}/events", run, worker);
        let mut params = Vec::new();
        if let Some(id) = after_id {
            params.push(format!("after_id={}", id));
        }
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        if !params.is_empty() {
            path = format!("{}?{}", path, params.join("&"));
        }

        self.client
            .get(&path)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    // -------------------------------------------------------------------------
    // Evals
    // -------------------------------------------------------------------------

    async fn list_evals(&self, run: &str) -> OrchestratorResult<Vec<Eval>> {
        self.client
            .get(&format!("/api/runtimes/{}/evals", run))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    // -------------------------------------------------------------------------
    // History
    // -------------------------------------------------------------------------

    async fn get_history(
        &self,
        run: &str,
        limit: Option<u32>,
    ) -> OrchestratorResult<Vec<HistoryEntry>> {
        let path = match limit {
            Some(n) => format!("/api/runtimes/{}/history?limit={}", run, n),
            None => format!("/api/runtimes/{}/history", run),
        };
        self.client
            .get(&path)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    // -------------------------------------------------------------------------
    // Configuration
    // -------------------------------------------------------------------------

    async fn get_config(&self) -> OrchestratorResult<ConfigResponse> {
        self.client
            .get("/api/config")
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn health(&self) -> OrchestratorResult<HealthResponse> {
        self.client
            .get("/health")
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    // -------------------------------------------------------------------------
    // Run Creation
    // -------------------------------------------------------------------------

    async fn start_run(&self, request: StartRunRequest) -> OrchestratorResult<RunDetail> {
        self.client
            .post("/api/runtimes/start", request)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn init_workspace(
        &self,
        runtime_name: &str,
        request: super::InitWorkspaceRequest,
    ) -> OrchestratorResult<super::InitWorkspaceResponse> {
        self.client
            .post(
                &format!("/api/runtimes/{}/workspace", runtime_name),
                request,
            )
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn spawn_single_worker(
        &self,
        runtime_name: &str,
        worker_name: &str,
        work_dir: &std::path::Path,
        resume_session_id: Option<&str>,
    ) -> OrchestratorResult<()> {
        let request = SpawnSingleWorkerRequest {
            work_dir: work_dir.to_string_lossy().to_string(),
            resume_session_id: resume_session_id.map(|s| s.to_string()),
        };
        let _: serde_json::Value = self
            .client
            .post(
                &format!(
                    "/api/runtimes/{}/workers/{}/spawn",
                    runtime_name, worker_name
                ),
                request,
            )
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn resume_worker(
        &self,
        runtime_name: &str,
        worker_name: &str,
        work_dir: &std::path::Path,
        resume_session_id: Option<&str>,
        state_handle: Option<&WorkerStateHandle>,
    ) -> OrchestratorResult<()> {
        let request = ResumeWorkerRequest {
            work_dir: work_dir.to_string_lossy().to_string(),
            resume_session_id: resume_session_id.map(|s| s.to_string()),
            state_handle: state_handle.cloned(),
        };
        let _: serde_json::Value = self
            .client
            .post(
                &format!(
                    "/api/runtimes/{}/workers/{}/resume",
                    runtime_name, worker_name
                ),
                request,
            )
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn create_project(
        &self,
        req: crate::backend::project::CreateProjectRequest,
    ) -> OrchestratorResult<crate::backend::project::Project> {
        self.client
            .post("/api/projects", req)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn get_project(&self, id: i64) -> OrchestratorResult<crate::backend::project::Project> {
        self.client
            .get(&format!("/api/projects/{}", id))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn get_project_by_name(
        &self,
        name: &str,
    ) -> OrchestratorResult<Option<crate::backend::project::Project>> {
        self.client
            .get(&format!("/api/projects/by-name/{}", name))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn list_projects(&self) -> OrchestratorResult<Vec<crate::backend::project::Project>> {
        self.client
            .get("/api/projects")
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn update_project(
        &self,
        id: i64,
        req: crate::backend::project::UpdateProjectRequest,
    ) -> OrchestratorResult<crate::backend::project::Project> {
        self.client
            .patch(&format!("/api/projects/{}", id), req)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn delete_project(&self, id: i64) -> OrchestratorResult<()> {
        self.client
            .delete(&format!("/api/projects/{}", id))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn list_route_runtimes(&self, project_id: i64) -> OrchestratorResult<Vec<RunSummary>> {
        self.client
            .get(&format!("/api/projects/{}/runs", project_id))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }
}
