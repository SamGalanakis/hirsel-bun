//! Remote orchestrator implementation
//!
//! Implements the Orchestrator trait using HTTP calls to a remote Hirsel server.

use async_trait::async_trait;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{
    AddTaskRequest, CreateRunRequest, CreateRunResponse, DeliverRunRequest, HealthResponse,
    Orchestrator, OrchestratorError, OrchestratorResult, ResumeRunRequest, ResumeWorkerRequest,
    SendMessageRequest, SpawnSingleWorkerRequest, SpawnWorkersRequest, SpawnWorkersResponse,
    StartRunRequest,
};
use crate::core::api_types::{
    ConfigResponse, Eval, HistoryEntry, Message, RunDetail, RunSummary, Task, ThreadSummary,
    Worker, WorkerEventsResponse,
};
use crate::core::draft::StartingPoint;
use crate::core::snapshot::WorkerStateHandle;

/// Remote orchestrator that communicates with a Hirsel server over HTTP
pub struct RemoteOrchestrator {
    client: Client,
    base_url: String,
    api_key: String,
}

impl RemoteOrchestrator {
    pub fn new(base_url: String, api_key: String) -> Self {
        // Remove trailing slash from base URL
        let base_url = base_url.trim_end_matches('/').to_string();

        Self {
            client: Client::new(),
            base_url,
            api_key,
        }
    }

    /// Make a GET request to the server
    async fn get<T: DeserializeOwned>(&self, path: &str) -> OrchestratorResult<T> {
        let url = format!("{}{}", self.base_url, path);

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::Http(format!(
                "HTTP {} from {}: {}",
                status, url, body
            )));
        }

        resp.json()
            .await
            .map_err(|e| OrchestratorError::Http(format!("JSON parse error: {}", e)))
    }

    /// Make a POST request to the server
    async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> OrchestratorResult<T> {
        let url = format!("{}{}", self.base_url, path);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::Http(format!(
                "HTTP {} from {}: {}",
                status, url, body
            )));
        }

        resp.json()
            .await
            .map_err(|e| OrchestratorError::Http(format!("JSON parse error: {}", e)))
    }

    /// Make a POST request that returns nothing
    async fn post_empty<B: Serialize>(&self, path: &str, body: &B) -> OrchestratorResult<()> {
        let url = format!("{}{}", self.base_url, path);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::Http(format!(
                "HTTP {} from {}: {}",
                status, url, body
            )));
        }

        Ok(())
    }

    /// Make a DELETE request to the server
    async fn delete(&self, path: &str) -> OrchestratorResult<()> {
        let url = format!("{}{}", self.base_url, path);

        let resp = self
            .client
            .delete(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::Http(format!(
                "HTTP {} from {}: {}",
                status, url, body
            )));
        }

        Ok(())
    }

    // =========================================================================
    // Server-Side Run Creation (not part of Orchestrator trait)
    // =========================================================================

    /// Create a new run on the remote server
    ///
    /// This sets up the run's state, spec, and initial worker.
    /// After this, call upload_files() and then spawn_workers().
    /// Download working directory tarball from the server
    ///
    /// Returns a gzipped tar archive of the run's work directory.
    pub async fn download_files(&self, run_name: &str) -> OrchestratorResult<Vec<u8>> {
        let url = format!(
            "{}/api/runs/{}/files",
            self.base_url,
            urlencoding::encode(run_name)
        );

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::Http(format!(
                "HTTP {} from {}: {}",
                status, url, body
            )));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| OrchestratorError::Http(format!("Failed to read response: {}", e)))?;

        Ok(bytes.to_vec())
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
    // Tasks
    // -------------------------------------------------------------------------

    async fn list_tasks(&self, run: &str) -> OrchestratorResult<Vec<Task>> {
        self.get(&format!("/api/runs/{}/tasks", urlencoding::encode(run)))
            .await
    }

    async fn add_task(&self, run: &str, content: &str) -> OrchestratorResult<Task> {
        let body = AddTaskRequest {
            content: content.to_string(),
        };
        self.post(
            &format!("/api/runs/{}/tasks", urlencoding::encode(run)),
            &body,
        )
        .await
    }

    async fn delete_task(&self, run: &str, task_id: &str) -> OrchestratorResult<()> {
        self.delete(&format!(
            "/api/runs/{}/tasks/{}",
            urlencoding::encode(run),
            urlencoding::encode(task_id)
        ))
        .await
    }

    async fn complete_task(&self, run: &str, task_id: &str) -> OrchestratorResult<()> {
        self.post_empty(
            &format!(
                "/api/runs/{}/tasks/{}/complete",
                urlencoding::encode(run),
                urlencoding::encode(task_id)
            ),
            &(),
        )
        .await
    }

    async fn reopen_task(&self, run: &str, task_id: &str) -> OrchestratorResult<()> {
        self.post_empty(
            &format!(
                "/api/runs/{}/tasks/{}/reopen",
                urlencoding::encode(run),
                urlencoding::encode(task_id)
            ),
            &(),
        )
        .await
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
        let url = format!(
            "{}/api/runs/{}/files",
            self.base_url,
            urlencoding::encode(run_name)
        );

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("Content-Type", "application/gzip")
            .body(tarball)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::Http(format!(
                "HTTP {} from {}: {}",
                status, url, body
            )));
        }

        Ok(())
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
    ) -> OrchestratorResult<SpawnWorkersResponse> {
        let body = SpawnWorkersRequest { count };
        self.post(
            &format!("/api/runs/{}/spawn", urlencoding::encode(run_name)),
            &body,
        )
        .await
    }

    async fn start_run(&self, request: StartRunRequest) -> OrchestratorResult<RunDetail> {
        // 1. Create tarball if starting from local folder
        let tarball = match &request.starting_point {
            StartingPoint::LocalFolder { path } => {
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
            StartingPoint::LocalFolder { .. } => None, // Files uploaded via tarball
            sp => Some(sp.clone()),
        };

        let create_request = CreateRunRequest {
            name: request.name.clone(),
            spec: request.spec,
            starting_point: starting_point_for_server,
            runner: request.runner,
            worker_scale: request.worker_scale,
            time_limit_minutes: request.time_limit_minutes.map(|m| m as u32),
            max_iterations: request.max_iterations.map(|m| m as u32),
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

        // 5. Spawn initial worker
        let _ = self.spawn_workers(&create_response.name, 1).await?;

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
