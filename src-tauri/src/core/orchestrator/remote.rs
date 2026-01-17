//! Remote orchestrator implementation
//!
//! Implements the Orchestrator trait using HTTP calls to a remote Hirsel server.

use async_trait::async_trait;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{
    AddTaskRequest, DeliverRunRequest, HealthResponse, Orchestrator, OrchestratorError,
    OrchestratorResult, ResumeRunRequest, SendMessageRequest, WorkerLogParams,
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

    async fn get_worker_log(
        &self,
        run: &str,
        worker: &str,
        lines: Option<usize>,
    ) -> OrchestratorResult<String> {
        #[derive(serde::Deserialize)]
        struct LogResponse {
            content: String,
        }

        let params = WorkerLogParams { lines };
        let query = serde_urlencoded::to_string(&params).unwrap_or_default();
        let path = format!(
            "/api/runs/{}/workers/{}/log?{}",
            urlencoding::encode(run),
            urlencoding::encode(worker),
            query
        );

        let resp: LogResponse = self.get(&path).await?;
        Ok(resp.content)
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
}
