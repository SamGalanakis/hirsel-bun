//! Remote orchestrator implementation
//!
//! Implements the Orchestrator trait using HTTP calls to a remote Hirsel server.

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{
    CreateRunRequest, CreateRunResponse, DeliverRunRequest, HealthResponse, Orchestrator,
    OrchestratorError, OrchestratorResult, ResumeRunRequest, ResumeWorkerRequest,
    SendMessageRequest, SpawnSingleWorkerRequest, SpawnWorkersRequest, SpawnWorkersResponse,
    StartRunRequest,
};
use crate::core::api_types::{
    ConfigResponse, Eval, HistoryEntry, Message, RunDetail, RunSummary, ThreadSummary, Worker,
    WorkerEventsResponse,
};
use crate::core::draft::StartingPoint;
use crate::core::http_client::{AuthenticatedClient, HttpError};
use crate::core::snapshot::WorkerStateHandle;

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
    /// Returns a gzipped tar archive of the run's work directory.
    pub async fn download_files(&self, run_name: &str) -> OrchestratorResult<Vec<u8>> {
        let path = format!("/api/runs/{}/files", urlencoding::encode(run_name));
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
        self.get("/api/runs").await
    }

    async fn get_run(&self, name: &str) -> OrchestratorResult<RunDetail> {
        self.get(&format!("/api/runs/{}", urlencoding::encode(name)))
            .await
    }

    async fn delete_run(&self, name: &str) -> OrchestratorResult<()> {
        self.delete(&format!("/api/runs/{}", urlencoding::encode(name)))
            .await
    }

    async fn pause_run(&self, name: &str) -> OrchestratorResult<()> {
        self.post_empty(
            &format!("/api/runs/{}/pause", urlencoding::encode(name)),
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
            &format!("/api/runs/{}/resume", urlencoding::encode(name)),
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
                &format!("/api/runs/{}/deliver", urlencoding::encode(name)),
                &body,
            )
            .await?;

        Ok(resp.branch)
    }

    // -------------------------------------------------------------------------
    // Workers
    // -------------------------------------------------------------------------

    async fn list_workers(&self, run: &str) -> OrchestratorResult<Vec<Worker>> {
        self.get(&format!("/api/runs/{}/workers", urlencoding::encode(run)))
            .await
    }

    async fn restart_worker(&self, run: &str, worker: &str) -> OrchestratorResult<()> {
        self.post_empty(
            &format!(
                "/api/runs/{}/workers/{}/restart",
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
            "/api/runs/{}/workers/{}/events?{}",
            urlencoding::encode(run),
            urlencoding::encode(worker),
            query
        );

        self.get(&path).await
    }

    // -------------------------------------------------------------------------
    // Messages
    // -------------------------------------------------------------------------

    async fn list_threads(&self, run: &str) -> OrchestratorResult<Vec<ThreadSummary>> {
        self.get(&format!("/api/runs/{}/threads", urlencoding::encode(run)))
            .await
    }

    async fn get_messages(&self, run: &str, thread: &str) -> OrchestratorResult<Vec<Message>> {
        self.get(&format!(
            "/api/runs/{}/threads/{}/messages",
            urlencoding::encode(run),
            urlencoding::encode(thread)
        ))
        .await
    }

    async fn send_message(
        &self,
        run: &str,
        thread: &str,
        content: &str,
    ) -> OrchestratorResult<Message> {
        let body = SendMessageRequest {
            content: content.to_string(),
        };
        self.post(
            &format!(
                "/api/runs/{}/threads/{}/messages",
                urlencoding::encode(run),
                urlencoding::encode(thread)
            ),
            &body,
        )
        .await
    }

    // -------------------------------------------------------------------------
    // Evals
    // -------------------------------------------------------------------------

    async fn list_evals(&self, run: &str) -> OrchestratorResult<Vec<Eval>> {
        self.get(&format!("/api/runs/{}/evals", urlencoding::encode(run)))
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
            Some(l) => format!("/api/runs/{}/history?limit={}", urlencoding::encode(run), l),
            None => format!("/api/runs/{}/history", urlencoding::encode(run)),
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

    async fn create_run(&self, request: CreateRunRequest) -> OrchestratorResult<CreateRunResponse> {
        self.post("/api/runs", &request).await
    }

    async fn upload_files(&self, run_name: &str, tarball: Vec<u8>) -> OrchestratorResult<()> {
        let path = format!("/api/runs/{}/files", urlencoding::encode(run_name));
        self.client
            .post_bytes(&path, tarball, "application/gzip")
            .await
            .map_err(Self::convert_error)
    }

    async fn init_workspace(
        &self,
        run_name: &str,
        request: super::InitWorkspaceRequest,
    ) -> OrchestratorResult<super::InitWorkspaceResponse> {
        self.post(
            &format!("/api/runs/{}/workspace", urlencoding::encode(run_name)),
            &request,
        )
        .await
    }

    async fn spawn_workers(
        &self,
        run_name: &str,
        count: u32,
        assigned_task_id: Option<String>,
    ) -> OrchestratorResult<SpawnWorkersResponse> {
        let body = SpawnWorkersRequest {
            count,
            assigned_task_id,
        };
        self.post(
            &format!("/api/runs/{}/spawn", urlencoding::encode(run_name)),
            &body,
        )
        .await
    }

    async fn start_run(&self, request: StartRunRequest) -> OrchestratorResult<RunDetail> {
        // 1. Create tarball if starting from local folder
        let tarball = match &request.starting_point {
            Some(StartingPoint::LocalFolder { path }) => {
                let project_path = std::path::Path::new(path);
                Some(create_project_tarball(project_path).map_err(|e| {
                    OrchestratorError::Other(format!("Failed to create tarball: {}", e))
                })?)
            }
            _ => None,
        };

        // 2. Convert to CreateRunRequest
        // For LocalFolder, we upload files separately, so don't include the local path
        let starting_point_for_server = match &request.starting_point {
            Some(StartingPoint::LocalFolder { .. }) => None, // Files uploaded via tarball
            sp => sp.clone(),
        };

        let create_request = CreateRunRequest {
            name: request.name.clone(),
            spec: request.spec,
            starting_point: starting_point_for_server,
            runner: request.runner,
            worker_scale: request.worker_scale,
            time_limit_minutes: request.time_limit_minutes.map(|m| m as u32),
            human_in_the_loop: request.human_in_the_loop,
            eval: request.eval,
            tailscale_oauth: request.tailscale_oauth,
        };

        // 3. Create run on remote server
        let create_response = self.create_run(create_request).await?;

        // 4. Upload files if tarball exists
        if let Some(tarball) = tarball {
            self.upload_files(&create_response.name, tarball).await?;
        }

        // 5. Spawn initial worker with scope task
        let _ = self
            .spawn_workers(&create_response.name, 1, Some("scope".to_string()))
            .await?;

        // 6. Return run detail
        self.get_run(&create_response.name).await
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
            .post(
                &format!(
                    "/api/runs/{}/workers/{}/spawn",
                    urlencoding::encode(run_name),
                    urlencoding::encode(worker_name)
                ),
                &request,
            )
            .await?;
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
            .post(
                &format!(
                    "/api/runs/{}/workers/{}/resume",
                    urlencoding::encode(run_name),
                    urlencoding::encode(worker_name)
                ),
                &request,
            )
            .await?;
        Ok(())
    }

    async fn create_project(
        &self,
        req: crate::core::project::CreateProjectRequest,
    ) -> OrchestratorResult<crate::core::project::Project> {
        self.post("/api/projects", &req).await
    }

    async fn get_project(&self, id: i64) -> OrchestratorResult<crate::core::project::Project> {
        self.get(&format!("/api/projects/{}", id)).await
    }

    async fn get_project_by_name(
        &self,
        name: &str,
    ) -> OrchestratorResult<Option<crate::core::project::Project>> {
        self.get(&format!("/api/projects/by-name/{}", name)).await
    }

    async fn list_projects(&self) -> OrchestratorResult<Vec<crate::core::project::Project>> {
        self.get("/api/projects").await
    }

    async fn update_project(
        &self,
        id: i64,
        req: crate::core::project::UpdateProjectRequest,
    ) -> OrchestratorResult<crate::core::project::Project> {
        self.patch(&format!("/api/projects/{}", id), &req).await
    }

    async fn delete_project(&self, id: i64) -> OrchestratorResult<()> {
        self.delete(&format!("/api/projects/{}", id)).await
    }

    async fn list_project_runs(&self, project_id: i64) -> OrchestratorResult<Vec<RunSummary>> {
        self.get(&format!("/api/projects/{}/runs", project_id))
            .await
    }
}

/// Create a tarball of a project directory, excluding common build artifacts
fn create_project_tarball(project_path: &std::path::Path) -> Result<Vec<u8>, std::io::Error> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use tar::Builder;
    use walkdir::WalkDir;

    let mut buffer = Vec::new();
    let encoder = GzEncoder::new(&mut buffer, Compression::fast());
    let mut builder = Builder::new(encoder);

    // Walk the project directory, excluding common build artifacts
    for entry in WalkDir::new(project_path)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_str().unwrap_or("");
            // Exclude common build/cache directories and files
            !matches!(
                name,
                "node_modules"
                    | "target"
                    | ".git"
                    | ".venv"
                    | "__pycache__"
                    | ".mypy_cache"
                    | ".pytest_cache"
                    | "dist"
                    | "build"
                    | ".next"
                    | ".nuxt"
                    | "coverage"
                    | ".turbo"
                    | ".vercel"
                    | ".netlify"
            )
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let relative_path = path.strip_prefix(project_path).unwrap_or(path);

        if path == project_path {
            continue; // Skip root directory itself
        }

        if path.is_file() {
            builder
                .append_path_with_name(path, relative_path)
                .map_err(std::io::Error::other)?;
        } else if path.is_dir() {
            builder
                .append_dir(relative_path, path)
                .map_err(std::io::Error::other)?;
        }
    }

    builder
        .into_inner()
        .map_err(std::io::Error::other)?
        .finish()
        .map_err(std::io::Error::other)?;

    Ok(buffer)
}
