//! Remote orchestrator implementation
//!
//! Implements the Orchestrator trait using HTTP calls to a remote Hirsel server.

use async_trait::async_trait;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{
    AddTaskRequest, CreateRunRequest, CreateRunResponse, DeliverRunRequest, HealthResponse,
    Orchestrator, OrchestratorError, OrchestratorResult, ResumeRunRequest, SendMessageRequest,
    SpawnWorkersRequest, SpawnWorkersResponse,
};
use crate::core::api_types::{
    ConfigResponse, Eval, HistoryEntry, Message, RunDetail, RunSummary, Task, ThreadSummary,
    Worker, WorkerEventsResponse,
};

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
    pub async fn create_run(
        &self,
        request: CreateRunRequest,
    ) -> OrchestratorResult<CreateRunResponse> {
        self.post("/api/runs", &request).await
    }

    /// Upload working directory as tarball to the server
    ///
    /// The tarball should be a gzipped tar archive of the project files.
    /// Workers will download and extract these files before starting.
    pub async fn upload_files(&self, run_name: &str, tarball: Vec<u8>) -> OrchestratorResult<()> {
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

    /// Spawn workers on the remote server
    ///
    /// Creates and starts the specified number of workers.
    /// The run must have files uploaded first.
    pub async fn spawn_workers(
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
}
