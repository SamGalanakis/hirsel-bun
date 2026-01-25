//! DaemonOrchestrator - Orchestrator that communicates with the local daemon
//!
//! This orchestrator sends all operations to the local hirsel daemon via Unix socket.
//! The daemon handles worker lifecycle, eval triggering, and other background operations.

use async_trait::async_trait;

use super::{
    AddTaskRequest, CreateRunRequest, CreateRunResponse, DeliverRunRequest, HealthResponse,
    Orchestrator, OrchestratorError, OrchestratorResult, ResumeRunRequest, ResumeWorkerRequest,
    SendMessageRequest, SpawnSingleWorkerRequest, SpawnWorkersRequest, SpawnWorkersResponse,
};
use crate::core::api_types::{
    ConfigResponse, Eval, HistoryEntry, Message, RunDetail, RunSummary, Task, ThreadSummary,
    Worker, WorkerEventsResponse,
};
use crate::core::snapshot::WorkerStateHandle;
use crate::daemon::DaemonClient;

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
            .get("/api/runs")
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn get_run(&self, name: &str) -> OrchestratorResult<RunDetail> {
        self.client
            .get(&format!("/api/runs/{}", name))
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
            .delete(&format!("/api/runs/{}", name))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn pause_run(&self, name: &str) -> OrchestratorResult<()> {
        let _: serde_json::Value = self
            .client
            .post_empty(&format!("/api/runs/{}/pause", name))
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
            .post(&format!("/api/runs/{}/resume", name), request)
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
            .post(&format!("/api/runs/{}/deliver", name), request)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(response.branch)
    }

    // -------------------------------------------------------------------------
    // Workers
    // -------------------------------------------------------------------------

    async fn list_workers(&self, run: &str) -> OrchestratorResult<Vec<Worker>> {
        self.client
            .get(&format!("/api/runs/{}/workers", run))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn restart_worker(&self, run: &str, worker: &str) -> OrchestratorResult<()> {
        let _: serde_json::Value = self
            .client
            .post_empty(&format!("/api/runs/{}/workers/{}/restart", run, worker))
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
        let mut path = format!("/api/runs/{}/workers/{}/events", run, worker);
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
    // Tasks
    // -------------------------------------------------------------------------

    async fn list_tasks(&self, run: &str) -> OrchestratorResult<Vec<Task>> {
        self.client
            .get(&format!("/api/runs/{}/tasks", run))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn add_task(&self, run: &str, content: &str) -> OrchestratorResult<Task> {
        let request = AddTaskRequest {
            content: content.to_string(),
        };
        self.client
            .post(&format!("/api/runs/{}/tasks", run), request)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn delete_task(&self, run: &str, task_id: &str) -> OrchestratorResult<()> {
        let _: serde_json::Value = self
            .client
            .delete(&format!("/api/runs/{}/tasks/{}", run, task_id))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn complete_task(&self, run: &str, task_id: &str) -> OrchestratorResult<()> {
        let _: serde_json::Value = self
            .client
            .post_empty(&format!("/api/runs/{}/tasks/{}/complete", run, task_id))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn reopen_task(&self, run: &str, task_id: &str) -> OrchestratorResult<()> {
        let _: serde_json::Value = self
            .client
            .post_empty(&format!("/api/runs/{}/tasks/{}/reopen", run, task_id))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Messages
    // -------------------------------------------------------------------------

    async fn list_threads(&self, run: &str) -> OrchestratorResult<Vec<ThreadSummary>> {
        self.client
            .get(&format!("/api/runs/{}/threads", run))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn get_messages(&self, run: &str, thread: &str) -> OrchestratorResult<Vec<Message>> {
        self.client
            .get(&format!("/api/runs/{}/threads/{}/messages", run, thread))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn send_message(
        &self,
        run: &str,
        thread: &str,
        content: &str,
    ) -> OrchestratorResult<Message> {
        let request = SendMessageRequest {
            content: content.to_string(),
        };
        self.client
            .post(
                &format!("/api/runs/{}/threads/{}/messages", run, thread),
                request,
            )
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    // -------------------------------------------------------------------------
    // Evals
    // -------------------------------------------------------------------------

    async fn list_evals(&self, run: &str) -> OrchestratorResult<Vec<Eval>> {
        self.client
            .get(&format!("/api/runs/{}/evals", run))
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
            Some(n) => format!("/api/runs/{}/history?limit={}", run, n),
            None => format!("/api/runs/{}/history", run),
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

    async fn create_run(&self, request: CreateRunRequest) -> OrchestratorResult<CreateRunResponse> {
        self.client
            .post("/api/runs", request)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn upload_files(&self, run_name: &str, tarball: Vec<u8>) -> OrchestratorResult<()> {
        self.client
            .post_bytes(&format!("/api/runs/{}/files", run_name), tarball)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn init_workspace(
        &self,
        run_name: &str,
        request: super::InitWorkspaceRequest,
    ) -> OrchestratorResult<super::InitWorkspaceResponse> {
        self.client
            .post(&format!("/api/runs/{}/workspace", run_name), request)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn spawn_workers(
        &self,
        run_name: &str,
        count: u32,
    ) -> OrchestratorResult<SpawnWorkersResponse> {
        let request = SpawnWorkersRequest { count };
        self.client
            .post(&format!("/api/runs/{}/spawn", run_name), request)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn spawn_single_worker(
        &self,
        run_name: &str,
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
                &format!("/api/runs/{}/workers/{}/spawn", run_name, worker_name),
                request,
            )
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn resume_worker(
        &self,
        run_name: &str,
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
                &format!("/api/runs/{}/workers/{}/resume", run_name, worker_name),
                request,
            )
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn create_project(
        &self,
        req: crate::core::project::CreateProjectRequest,
    ) -> OrchestratorResult<crate::core::project::Project> {
        self.client
            .post("/api/projects", req)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn get_project(&self, id: i64) -> OrchestratorResult<crate::core::project::Project> {
        self.client
            .get(&format!("/api/projects/{}", id))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn get_project_by_name(
        &self,
        name: &str,
    ) -> OrchestratorResult<Option<crate::core::project::Project>> {
        self.client
            .get(&format!("/api/projects/by-name/{}", name))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn list_projects(&self) -> OrchestratorResult<Vec<crate::core::project::Project>> {
        self.client
            .get("/api/projects")
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }

    async fn update_project(
        &self,
        id: i64,
        req: crate::core::project::UpdateProjectRequest,
    ) -> OrchestratorResult<crate::core::project::Project> {
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

    async fn list_project_runs(&self, project_id: i64) -> OrchestratorResult<Vec<RunSummary>> {
        self.client
            .get(&format!("/api/projects/{}/runs", project_id))
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))
    }
}
