//! Remote orchestrator implementation
//!
//! Implements the Orchestrator trait using HTTP calls to a remote Hirsel server.

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{
    DeliverRunRequest, HealthResponse, Orchestrator, OrchestratorError, OrchestratorResult,
    ResumeRunRequest, ResumeWorkerRequest, SpawnSingleWorkerRequest, StartRunRequest,
};
use crate::backend::api_types::{
    ConfigResponse, Eval, HistoryEntry, RunDetail, RunSummary, Worker, WorkerEventsResponse,
};
use crate::backend::http_client::{AuthenticatedClient, HttpError};
use crate::backend::snapshot::WorkerStateHandle;

/// Remote orchestrator that communicates with a Hirsel server over HTTP
pub struct RemoteOrchestrator {
    client: AuthenticatedClient,
}

impl RemoteOrchestrator {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            client: AuthenticatedClient::new(base_url, api_key),
        }
    }

    /// Wrapper to convert HttpError to OrchestratorError
    fn convert_error(e: HttpError) -> OrchestratorError {
        match e {
            HttpError::Response { status, url, body } => {
                OrchestratorError::Http(format!("HTTP {} from {}: {}", status, url, body))
            }
            HttpError::Request(e) => OrchestratorError::Http(e.to_string()),
            HttpError::Parse(msg) => OrchestratorError::Http(msg),
        }
    }

    /// Make a GET request to the server
    async fn get<T: DeserializeOwned>(&self, path: &str) -> OrchestratorResult<T> {
        self.client.get(path).await.map_err(Self::convert_error)
    }

    /// Make a POST request to the server
    async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> OrchestratorResult<T> {
        self.client
            .post(path, body)
            .await
            .map_err(Self::convert_error)
    }

    /// Make a POST request that returns nothing
    async fn post_empty<B: Serialize>(&self, path: &str, body: &B) -> OrchestratorResult<()> {
        self.client
            .post_empty(path, body)
            .await
            .map_err(Self::convert_error)
    }

    /// Make a DELETE request to the server
    async fn delete(&self, path: &str) -> OrchestratorResult<()> {
        self.client.delete(path).await.map_err(Self::convert_error)
    }

    /// Make a PATCH request to the server
    async fn patch<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> OrchestratorResult<T> {
        self.client
            .patch(path, body)
            .await
            .map_err(Self::convert_error)
    }

    // =========================================================================
    // Server-Side Run Creation (not part of Orchestrator trait)
    // =========================================================================

    /// Download working directory tarball from the server
    ///
    /// Returns a gzipped tar archive of the runtime workspace.
    pub async fn download_files(&self, runtime_name: &str) -> OrchestratorResult<Vec<u8>> {
        let path = format!("/api/runtimes/{}/files", urlencoding::encode(runtime_name));
        self.client
            .get_bytes(&path)
            .await
            .map_err(Self::convert_error)
    }
}

#[async_trait]
impl Orchestrator for RemoteOrchestrator {
    // -------------------------------------------------------------------------
    // Run Management
    // -------------------------------------------------------------------------

    async fn list_runs(&self) -> OrchestratorResult<Vec<RunSummary>> {
        self.get("/api/runtimes").await
    }

    async fn get_run(&self, name: &str) -> OrchestratorResult<RunDetail> {
        self.get(&format!("/api/runtimes/{}", urlencoding::encode(name)))
            .await
    }

    async fn delete_run(&self, name: &str) -> OrchestratorResult<()> {
        self.delete(&format!("/api/runtimes/{}", urlencoding::encode(name)))
            .await
    }

    async fn pause_run(&self, name: &str) -> OrchestratorResult<()> {
        self.post_empty(
            &format!("/api/runtimes/{}/pause", urlencoding::encode(name)),
            &(),
        )
        .await
    }

    async fn resume_run(
        &self,
        name: &str,
        time_limit_minutes: Option<u32>,
    ) -> OrchestratorResult<()> {
        let body = ResumeRunRequest { time_limit_minutes };
        self.post_empty(
            &format!("/api/runtimes/{}/resume", urlencoding::encode(name)),
            &body,
        )
        .await
    }

    async fn deliver_run(&self, name: &str, branch: Option<String>) -> OrchestratorResult<String> {
        #[derive(serde::Deserialize)]
        struct DeliverResponse {
            branch: String,
        }

        let body = DeliverRunRequest { branch };
        let resp: DeliverResponse = self
            .post(
                &format!("/api/runtimes/{}/deliver", urlencoding::encode(name)),
                &body,
            )
            .await?;

        Ok(resp.branch)
    }

    // -------------------------------------------------------------------------
    // Workers
    // -------------------------------------------------------------------------

    async fn list_workers(&self, run: &str) -> OrchestratorResult<Vec<Worker>> {
        self.get(&format!(
            "/api/runtimes/{}/workers",
            urlencoding::encode(run)
        ))
        .await
    }

    async fn restart_worker(&self, run: &str, worker: &str) -> OrchestratorResult<()> {
        self.post_empty(
            &format!(
                "/api/runtimes/{}/workers/{}/restart",
                urlencoding::encode(run),
                urlencoding::encode(worker)
            ),
            &(),
        )
        .await
    }

    async fn get_worker_events(
        &self,
        run: &str,
        worker: &str,
        after_id: Option<i64>,
        limit: Option<i64>,
    ) -> OrchestratorResult<WorkerEventsResponse> {
        #[derive(serde::Serialize)]
        struct EventParams {
            #[serde(skip_serializing_if = "Option::is_none")]
            after_id: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<i64>,
        }

        let params = EventParams { after_id, limit };
        let query = serde_urlencoded::to_string(&params).unwrap_or_default();
        let path = format!(
            "/api/runtimes/{}/workers/{}/events?{}",
            urlencoding::encode(run),
            urlencoding::encode(worker),
            query
        );

        self.get(&path).await
    }

    // -------------------------------------------------------------------------
    // Evals
    // -------------------------------------------------------------------------

    async fn list_evals(&self, run: &str) -> OrchestratorResult<Vec<Eval>> {
        self.get(&format!("/api/runtimes/{}/evals", urlencoding::encode(run)))
            .await
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
            Some(l) => format!(
                "/api/runtimes/{}/history?limit={}",
                urlencoding::encode(run),
                l
            ),
            None => format!("/api/runtimes/{}/history", urlencoding::encode(run)),
        };
        self.get(&path).await
    }

    // -------------------------------------------------------------------------
    // Configuration
    // -------------------------------------------------------------------------

    async fn get_config(&self) -> OrchestratorResult<ConfigResponse> {
        self.get("/api/config").await
    }

    async fn health(&self) -> OrchestratorResult<HealthResponse> {
        self.get("/health").await
    }

    // -------------------------------------------------------------------------
    // Run Creation
    // -------------------------------------------------------------------------

    async fn init_workspace(
        &self,
        runtime_name: &str,
        request: super::InitWorkspaceRequest,
    ) -> OrchestratorResult<super::InitWorkspaceResponse> {
        self.post(
            &format!(
                "/api/runtimes/{}/workspace",
                urlencoding::encode(runtime_name)
            ),
            &request,
        )
        .await
    }

    async fn start_run(&self, request: StartRunRequest) -> OrchestratorResult<RunDetail> {
        self.post("/api/runtimes/start", &request).await
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
            .post(
                &format!(
                    "/api/runtimes/{}/workers/{}/spawn",
                    urlencoding::encode(runtime_name),
                    urlencoding::encode(worker_name)
                ),
                &request,
            )
            .await?;
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
            .post(
                &format!(
                    "/api/runtimes/{}/workers/{}/resume",
                    urlencoding::encode(runtime_name),
                    urlencoding::encode(worker_name)
                ),
                &request,
            )
            .await?;
        Ok(())
    }

    async fn create_project(
        &self,
        req: crate::backend::project::CreateProjectRequest,
    ) -> OrchestratorResult<crate::backend::project::Project> {
        self.post("/api/projects", &req).await
    }

    async fn get_project(&self, id: i64) -> OrchestratorResult<crate::backend::project::Project> {
        self.get(&format!("/api/projects/{}", id)).await
    }

    async fn get_project_by_name(
        &self,
        name: &str,
    ) -> OrchestratorResult<Option<crate::backend::project::Project>> {
        self.get(&format!("/api/projects/by-name/{}", name)).await
    }

    async fn list_projects(&self) -> OrchestratorResult<Vec<crate::backend::project::Project>> {
        self.get("/api/projects").await
    }

    async fn update_project(
        &self,
        id: i64,
        req: crate::backend::project::UpdateProjectRequest,
    ) -> OrchestratorResult<crate::backend::project::Project> {
        self.patch(&format!("/api/projects/{}", id), &req).await
    }

    async fn delete_project(&self, id: i64) -> OrchestratorResult<()> {
        self.delete(&format!("/api/projects/{}", id)).await
    }

    async fn list_route_runtimes(&self, project_id: i64) -> OrchestratorResult<Vec<RunSummary>> {
        self.get(&format!("/api/projects/{}/runs", project_id))
            .await
    }
}
