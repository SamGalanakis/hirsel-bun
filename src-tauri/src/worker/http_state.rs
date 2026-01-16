//! HTTP client for remote workers connecting to coordinator API.
//!
//! This provides a StateAccess-like interface over HTTP for remote workers
//! that connect to the coordinator via SSH tunnel.

use std::time::Duration;

use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

use crate::core::state::{Message, Status, Task, Worker, WorkerStatus};

// =============================================================================
// Common Response Types
// =============================================================================

/// Generic success response from API endpoints
#[derive(Deserialize)]
struct SuccessResponse {
    #[allow(dead_code)] // Field exists for API compatibility, value not checked
    success: bool,
}

// =============================================================================
// Errors
// =============================================================================

#[derive(Debug, Error)]
pub enum HttpStateError {
    #[error("Connection failed: {0}")]
    Connection(#[from] reqwest::Error),

    #[error("Operation failed: {status} - {message}")]
    Operation { status: u16, message: String },

    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

pub type HttpStateResult<T> = Result<T, HttpStateError>;

// =============================================================================
// HTTP State Client
// =============================================================================

/// HTTP client implementation for remote workers.
///
/// Connects to the coordinator's HTTP API via SSH tunnel.
pub struct HttpState {
    base_url: String,
    worker_name: String,
    client: Client,
}

impl HttpState {
    /// Create a new HTTP state client.
    pub fn new(base_url: &str, worker_name: &str, timeout_secs: u64) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            worker_name: worker_name.to_string(),
            client,
        }
    }

    // =========================================================================
    // Internal HTTP methods
    // =========================================================================

    async fn get<T: DeserializeOwned>(&self, endpoint: &str) -> HttpStateResult<T> {
        let url = format!("{}{}", self.base_url, endpoint);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(HttpStateError::Operation { status, message });
        }

        response
            .json()
            .await
            .map_err(|e| HttpStateError::InvalidResponse(e.to_string()))
    }

    async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> HttpStateResult<T> {
        let url = format!("{}{}", self.base_url, endpoint);
        let response = self.client.post(&url).json(body).send().await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(HttpStateError::Operation { status, message });
        }

        response
            .json()
            .await
            .map_err(|e| HttpStateError::InvalidResponse(e.to_string()))
    }

    async fn delete<T: DeserializeOwned>(&self, endpoint: &str) -> HttpStateResult<T> {
        let url = format!("{}{}", self.base_url, endpoint);
        let response = self.client.delete(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(HttpStateError::Operation { status, message });
        }

        response
            .json()
            .await
            .map_err(|e| HttpStateError::InvalidResponse(e.to_string()))
    }

    // =========================================================================
    // Health check
    // =========================================================================

    pub async fn health_check(&self) -> HttpStateResult<bool> {
        #[derive(Deserialize)]
        struct Health {
            status: String,
        }
        let result: Health = self.get("/health").await?;
        Ok(result.status == "ok")
    }

    // =========================================================================
    // Run status
    // =========================================================================

    pub async fn get_status(&self) -> HttpStateResult<Status> {
        #[derive(Deserialize)]
        struct StatusResponse {
            status: String,
        }
        let result: StatusResponse = self.get("/status").await?;
        Status::from_str(&result.status).ok_or_else(|| {
            HttpStateError::InvalidResponse(format!("Invalid status: {}", result.status))
        })
    }

    pub async fn set_status(&self, status: Status) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct StatusRequest {
            status: String,
        }
        let _: SuccessResponse = self
            .post(
                "/status",
                &StatusRequest {
                    status: status.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    // =========================================================================
    // Task operations
    // =========================================================================

    pub async fn get_tasks(&self) -> HttpStateResult<Vec<Task>> {
        #[derive(Deserialize)]
        struct TasksResponse {
            tasks: Vec<Task>,
        }
        let result: TasksResponse = self.get("/tasks").await?;
        Ok(result.tasks)
    }

    pub async fn get_claimable_tasks(&self) -> HttpStateResult<Vec<Task>> {
        #[derive(Deserialize)]
        struct TasksResponse {
            tasks: Vec<Task>,
        }
        let result: TasksResponse = self.get("/tasks/claimable").await?;
        Ok(result.tasks)
    }

    pub async fn get_task(&self, task_id: &str) -> HttpStateResult<Option<Task>> {
        #[derive(Deserialize)]
        struct TaskResponse {
            task: Option<Task>,
        }
        let result: TaskResponse = self.get(&format!("/tasks/{}", task_id)).await?;
        Ok(result.task)
    }

    pub async fn claim_task(&self, task_id: &str, worker_name: &str) -> HttpStateResult<bool> {
        #[derive(Serialize)]
        struct ClaimRequest {
            worker_name: String,
        }
        let result: SuccessResponse = self
            .post(
                &format!("/tasks/{}/claim", task_id),
                &ClaimRequest {
                    worker_name: worker_name.to_string(),
                },
            )
            .await?;
        Ok(result.success)
    }

    pub async fn complete_task(&self, task_id: &str, worker_name: &str) -> HttpStateResult<bool> {
        #[derive(Serialize)]
        struct CompleteRequest {
            worker_name: String,
        }
        let result: SuccessResponse = self
            .post(
                &format!("/tasks/{}/complete", task_id),
                &CompleteRequest {
                    worker_name: worker_name.to_string(),
                },
            )
            .await?;
        Ok(result.success)
    }

    pub async fn unclaim_task(&self, task_id: &str, worker_name: &str) -> HttpStateResult<bool> {
        #[derive(Serialize)]
        struct UncompleteRequest {
            worker_name: String,
        }
        let result: SuccessResponse = self
            .post(
                &format!("/tasks/{}/unclaim", task_id),
                &UncompleteRequest {
                    worker_name: worker_name.to_string(),
                },
            )
            .await?;
        Ok(result.success)
    }

    pub async fn add_task(
        &self,
        task_id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<Vec<&str>>,
    ) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct TaskRequest {
            task_id: String,
            name: String,
            parent_id: Option<String>,
            blocked_by: Option<Vec<String>>,
        }
        let _: SuccessResponse = self
            .post(
                "/tasks",
                &TaskRequest {
                    task_id: task_id.to_string(),
                    name: name.to_string(),
                    parent_id: parent_id.map(|s| s.to_string()),
                    blocked_by: blocked_by.map(|v| v.iter().map(|s| s.to_string()).collect()),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn set_task_pending_done(&self, task_id: &str) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct Empty {}
        let _: SuccessResponse = self
            .post(&format!("/tasks/{}/pending_done", task_id), &Empty {})
            .await?;
        Ok(())
    }

    pub async fn clear_task_pending_done(&self, task_id: &str) -> HttpStateResult<()> {
        let _: SuccessResponse = self
            .delete(&format!("/tasks/{}/pending_done", task_id))
            .await?;
        Ok(())
    }

    // =========================================================================
    // Worker operations
    // =========================================================================

    pub async fn get_workers(&self) -> HttpStateResult<Vec<Worker>> {
        #[derive(Deserialize)]
        struct WorkersResponse {
            workers: Vec<Worker>,
        }
        let result: WorkersResponse = self.get("/workers").await?;
        Ok(result.workers)
    }

    pub async fn get_worker(&self, name: &str) -> HttpStateResult<Option<Worker>> {
        #[derive(Deserialize)]
        struct WorkerResponse {
            worker: Option<Worker>,
        }
        let result: WorkerResponse = self.get(&format!("/workers/{}", name)).await?;
        Ok(result.worker)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_worker(
        &self,
        name: &str,
        pid: Option<i64>,
        session_id: Option<&str>,
        status: Option<WorkerStatus>,
        waiting_thread: Option<&str>,
        needs_restart: Option<bool>,
        last_heartbeat: Option<&str>,
    ) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct UpdateRequest {
            #[serde(skip_serializing_if = "Option::is_none")]
            pid: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            session_id: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            status: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            waiting_thread: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            needs_restart: Option<bool>,
            #[serde(skip_serializing_if = "Option::is_none")]
            last_heartbeat: Option<String>,
        }
        let _: SuccessResponse = self
            .post(
                &format!("/workers/{}/update", name),
                &UpdateRequest {
                    pid,
                    session_id: session_id.map(|s| s.to_string()),
                    status: status.map(|s| s.to_string()),
                    waiting_thread: waiting_thread.map(|s| s.to_string()),
                    needs_restart,
                    last_heartbeat: last_heartbeat.map(|s| s.to_string()),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn heartbeat(&self) -> HttpStateResult<Status> {
        #[derive(Deserialize)]
        struct HeartbeatResponse {
            status: String,
        }
        #[derive(Serialize)]
        struct Empty {}
        let result: HeartbeatResponse = self
            .post(
                &format!("/workers/{}/heartbeat", self.worker_name),
                &Empty {},
            )
            .await?;
        Status::from_str(&result.status).ok_or_else(|| {
            HttpStateError::InvalidResponse(format!("Invalid status: {}", result.status))
        })
    }

    pub async fn get_claimed_task(&self, worker_name: &str) -> HttpStateResult<Option<Task>> {
        #[derive(Deserialize)]
        struct TaskResponse {
            task: Option<Task>,
        }
        let result: TaskResponse = self
            .get(&format!("/workers/{}/claimed_task", worker_name))
            .await?;
        Ok(result.task)
    }

    // =========================================================================
    // Message operations
    // =========================================================================

    pub async fn add_message(
        &self,
        thread: &str,
        sender: &str,
        content: &str,
        waiting: bool,
    ) -> HttpStateResult<i64> {
        #[derive(Serialize)]
        struct MessageRequest {
            thread: String,
            sender: String,
            content: String,
            waiting: bool,
        }
        #[derive(Deserialize)]
        struct MessageResponse {
            id: i64,
        }
        let result: MessageResponse = self
            .post(
                "/messages",
                &MessageRequest {
                    thread: thread.to_string(),
                    sender: sender.to_string(),
                    content: content.to_string(),
                    waiting,
                },
            )
            .await?;
        Ok(result.id)
    }

    pub async fn get_messages(&self, thread: &str, limit: i64) -> HttpStateResult<Vec<Message>> {
        #[derive(Deserialize)]
        struct MessagesResponse {
            messages: Vec<Message>,
        }
        let result: MessagesResponse = self
            .get(&format!("/messages/{}?limit={}", thread, limit))
            .await?;
        Ok(result.messages)
    }

    pub async fn get_unread_messages(
        &self,
        thread: &str,
        reader: &str,
    ) -> HttpStateResult<Vec<Message>> {
        #[derive(Deserialize)]
        struct MessagesResponse {
            messages: Vec<Message>,
        }
        let result: MessagesResponse = self
            .get(&format!("/messages/{}/unread/{}", thread, reader))
            .await?;
        Ok(result.messages)
    }

    pub async fn mark_messages_read(
        &self,
        thread: &str,
        reader: &str,
        up_to_id: Option<i64>,
    ) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct MarkReadRequest {
            reader: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            up_to_id: Option<i64>,
        }
        let _: SuccessResponse = self
            .post(
                &format!("/messages/{}/mark_read", thread),
                &MarkReadRequest {
                    reader: reader.to_string(),
                    up_to_id,
                },
            )
            .await?;
        Ok(())
    }

    // =========================================================================
    // Config operations
    // =========================================================================

    pub async fn get_request(&self) -> HttpStateResult<Option<String>> {
        #[derive(Deserialize)]
        struct RequestResponse {
            request: Option<String>,
        }
        let result: RequestResponse = self.get("/config/request").await?;
        Ok(result.request)
    }

    pub async fn get_project_path(&self) -> HttpStateResult<Option<String>> {
        #[derive(Deserialize)]
        struct PathResponse {
            project_path: Option<String>,
        }
        let result: PathResponse = self.get("/config/project_path").await?;
        Ok(result.project_path)
    }

    pub async fn get_human_in_the_loop(&self) -> HttpStateResult<bool> {
        #[derive(Deserialize)]
        struct HitlResponse {
            enabled: bool,
        }
        let result: HitlResponse = self.get("/config/human_in_the_loop").await?;
        Ok(result.enabled)
    }

    pub async fn set_waiting_reason(&self, reason: Option<&str>) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct ReasonRequest {
            reason: Option<String>,
        }
        let _: SuccessResponse = self
            .post(
                "/config/waiting_reason",
                &ReasonRequest {
                    reason: reason.map(|s| s.to_string()),
                },
            )
            .await?;
        Ok(())
    }

    // =========================================================================
    // Time tracking
    // =========================================================================

    pub async fn get_time_limit_minutes(&self) -> HttpStateResult<Option<i64>> {
        #[derive(Deserialize)]
        struct TimeResponse {
            minutes: Option<i64>,
        }
        let result: TimeResponse = self.get("/time/limit").await?;
        Ok(result.minutes)
    }

    pub async fn is_time_expired(&self) -> HttpStateResult<bool> {
        #[derive(Deserialize)]
        struct ExpiredResponse {
            expired: bool,
        }
        let result: ExpiredResponse = self.get("/time/expired").await?;
        Ok(result.expired)
    }

    // =========================================================================
    // Iteration tracking
    // =========================================================================

    pub async fn get_iteration_count(&self) -> HttpStateResult<i64> {
        #[derive(Deserialize)]
        struct CountResponse {
            count: i64,
        }
        let result: CountResponse = self.get("/iterations/count").await?;
        Ok(result.count)
    }

    pub async fn increment_iteration(&self) -> HttpStateResult<i64> {
        #[derive(Serialize)]
        struct Empty {}
        #[derive(Deserialize)]
        struct CountResponse {
            count: i64,
        }
        let result: CountResponse = self.post("/iterations/increment", &Empty {}).await?;
        Ok(result.count)
    }

    pub async fn get_max_iterations(&self) -> HttpStateResult<Option<i64>> {
        #[derive(Deserialize)]
        struct MaxResponse {
            max: Option<i64>,
        }
        let result: MaxResponse = self.get("/iterations/max").await?;
        Ok(result.max)
    }

    // =========================================================================
    // Additional methods for StateAccess trait
    // =========================================================================

    pub async fn delete_task(&self, task_id: &str) -> HttpStateResult<()> {
        let _: SuccessResponse = self.delete(&format!("/tasks/{}", task_id)).await?;
        Ok(())
    }

    pub async fn is_task_blocked(&self, task_id: &str) -> HttpStateResult<bool> {
        #[derive(Deserialize)]
        struct BlockedResponse {
            blocked: bool,
        }
        let result: BlockedResponse = self.get(&format!("/tasks/{}/blocked", task_id)).await?;
        Ok(result.blocked)
    }

    pub async fn get_blockers(&self, task_id: &str) -> HttpStateResult<Vec<String>> {
        #[derive(Deserialize)]
        struct BlockersResponse {
            blockers: Vec<String>,
        }
        let result: BlockersResponse = self.get(&format!("/tasks/{}/blockers", task_id)).await?;
        Ok(result.blockers)
    }

    pub async fn has_children(&self, task_id: &str) -> HttpStateResult<bool> {
        #[derive(Deserialize)]
        struct ChildrenResponse {
            has_children: bool,
        }
        let result: ChildrenResponse = self
            .get(&format!("/tasks/{}/has_children", task_id))
            .await?;
        Ok(result.has_children)
    }

    pub async fn get_children(&self, task_id: &str) -> HttpStateResult<Vec<Task>> {
        #[derive(Deserialize)]
        struct ChildrenResponse {
            children: Vec<Task>,
        }
        let result: ChildrenResponse = self.get(&format!("/tasks/{}/children", task_id)).await?;
        Ok(result.children)
    }

    pub async fn reopen_task(&self, task_id: &str) -> HttpStateResult<bool> {
        #[derive(Serialize)]
        struct Empty {}
        let result: SuccessResponse = self
            .post(&format!("/tasks/{}/reopen", task_id), &Empty {})
            .await?;
        Ok(result.success)
    }

    pub async fn set_task_tokens(&self, task_id: &str, tokens: i64) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct TokensRequest {
            tokens: i64,
        }
        let _: SuccessResponse = self
            .post(
                &format!("/tasks/{}/tokens", task_id),
                &TokensRequest { tokens },
            )
            .await?;
        Ok(())
    }

    pub async fn get_active_workers(&self) -> HttpStateResult<Vec<Worker>> {
        #[derive(Deserialize)]
        struct WorkersResponse {
            workers: Vec<Worker>,
        }
        let result: WorkersResponse = self.get("/workers/active").await?;
        Ok(result.workers)
    }

    pub async fn all_workers_done(&self) -> HttpStateResult<bool> {
        #[derive(Deserialize)]
        struct DoneResponse {
            all_done: bool,
        }
        let result: DoneResponse = self.get("/workers/all_done").await?;
        Ok(result.all_done)
    }

    pub async fn get_all_unread_messages(&self, reader: &str) -> HttpStateResult<Vec<Message>> {
        #[derive(Deserialize)]
        struct MessagesResponse {
            messages: Vec<Message>,
        }
        let result: MessagesResponse = self.get(&format!("/messages/unread/{}", reader)).await?;
        Ok(result.messages)
    }

    pub async fn get_threads(&self) -> HttpStateResult<Vec<String>> {
        #[derive(Deserialize)]
        struct ThreadsResponse {
            threads: Vec<String>,
        }
        let result: ThreadsResponse = self.get("/messages/threads").await?;
        Ok(result.threads)
    }

    pub async fn get_time_info(&self) -> HttpStateResult<Option<crate::core::state::TimeInfo>> {
        #[derive(Deserialize)]
        struct TimeInfoResponse {
            time_info: Option<crate::core::state::TimeInfo>,
        }
        let result: TimeInfoResponse = self.get("/time/info").await?;
        Ok(result.time_info)
    }
}

// =============================================================================
// StateAccess Implementation for HttpState
// =============================================================================

use crate::core::state::{Eval, HistoryEntry, TimeInfo, WorkerUpdate};
use crate::core::state_access::{StateAccess, StateAccessError, StateAccessResult};
use async_trait::async_trait;

impl From<HttpStateError> for StateAccessError {
    fn from(e: HttpStateError) -> Self {
        match e {
            HttpStateError::Connection(e) => StateAccessError::Connection(e.to_string()),
            HttpStateError::Operation { status, message } => {
                StateAccessError::Http(format!("HTTP {}: {}", status, message))
            }
            HttpStateError::InvalidResponse(msg) => StateAccessError::Http(msg),
        }
    }
}

#[async_trait(?Send)]
impl StateAccess for HttpState {
    async fn status(&self) -> StateAccessResult<Status> {
        Ok(self.get_status().await?)
    }

    async fn set_status(&self, status: Status) -> StateAccessResult<()> {
        Ok(HttpState::set_status(self, status).await?)
    }

    async fn add_task(
        &self,
        task_id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
    ) -> StateAccessResult<()> {
        let blocked_by_vec = blocked_by.map(|b| b.to_vec());
        Ok(HttpState::add_task(self, task_id, name, parent_id, blocked_by_vec).await?)
    }

    async fn get_tasks(&self) -> StateAccessResult<Vec<Task>> {
        Ok(HttpState::get_tasks(self).await?)
    }

    async fn get_task(&self, task_id: &str) -> StateAccessResult<Option<Task>> {
        Ok(HttpState::get_task(self, task_id).await?)
    }

    async fn claim_task(&self, task_id: &str, worker_name: &str) -> StateAccessResult<bool> {
        Ok(HttpState::claim_task(self, task_id, worker_name).await?)
    }

    async fn complete_task(&self, task_id: &str, worker_name: &str) -> StateAccessResult<bool> {
        Ok(HttpState::complete_task(self, task_id, worker_name).await?)
    }

    async fn unclaim_task(&self, task_id: &str, worker_name: &str) -> StateAccessResult<bool> {
        Ok(HttpState::unclaim_task(self, task_id, worker_name).await?)
    }

    async fn get_claimed_task(&self, worker_name: &str) -> StateAccessResult<Option<Task>> {
        Ok(HttpState::get_claimed_task(self, worker_name).await?)
    }

    async fn get_claimable_tasks(&self) -> StateAccessResult<Vec<Task>> {
        Ok(HttpState::get_claimable_tasks(self).await?)
    }

    async fn delete_task(&self, task_id: &str) -> StateAccessResult<()> {
        Ok(HttpState::delete_task(self, task_id).await?)
    }

    async fn is_task_blocked(&self, task_id: &str) -> StateAccessResult<bool> {
        Ok(HttpState::is_task_blocked(self, task_id).await?)
    }

    async fn get_blockers(&self, task_id: &str) -> StateAccessResult<Vec<String>> {
        Ok(HttpState::get_blockers(self, task_id).await?)
    }

    async fn has_children(&self, task_id: &str) -> StateAccessResult<bool> {
        Ok(HttpState::has_children(self, task_id).await?)
    }

    async fn get_children(&self, task_id: &str) -> StateAccessResult<Vec<Task>> {
        Ok(HttpState::get_children(self, task_id).await?)
    }

    async fn set_task_pending_done(&self, task_id: &str) -> StateAccessResult<()> {
        Ok(HttpState::set_task_pending_done(self, task_id).await?)
    }

    async fn clear_task_pending_done(&self, task_id: &str) -> StateAccessResult<()> {
        Ok(HttpState::clear_task_pending_done(self, task_id).await?)
    }

    async fn reopen_task(&self, task_id: &str) -> StateAccessResult<bool> {
        Ok(HttpState::reopen_task(self, task_id).await?)
    }

    async fn set_task_tokens(&self, task_id: &str, tokens: i64) -> StateAccessResult<()> {
        Ok(HttpState::set_task_tokens(self, task_id, tokens).await?)
    }

    async fn add_worker(
        &self,
        _name: &str,
        _work_dir: &str,
        _location: &str,
    ) -> StateAccessResult<Option<Worker>> {
        // Workers are registered by the coordinator, not by remote workers
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot add workers".to_string(),
        ))
    }

    async fn get_worker(&self, name: &str) -> StateAccessResult<Option<Worker>> {
        Ok(HttpState::get_worker(self, name).await?)
    }

    async fn get_workers(&self) -> StateAccessResult<Vec<Worker>> {
        Ok(HttpState::get_workers(self).await?)
    }

    async fn update_worker(&self, name: &str, updates: WorkerUpdate) -> StateAccessResult<()> {
        Ok(HttpState::update_worker(
            self,
            name,
            updates.pid,
            updates.session_id.as_deref(),
            updates.status,
            updates.waiting_thread.as_deref(),
            updates.needs_restart,
            updates.last_heartbeat.as_deref(),
        )
        .await?)
    }

    async fn get_active_workers(&self) -> StateAccessResult<Vec<Worker>> {
        Ok(HttpState::get_active_workers(self).await?)
    }

    async fn all_workers_done(&self) -> StateAccessResult<bool> {
        Ok(HttpState::all_workers_done(self).await?)
    }

    async fn pause_all_workers(&self, _reason: &str) -> StateAccessResult<()> {
        // Coordinator-only operation
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot pause all workers".to_string(),
        ))
    }

    async fn resume_all_workers(&self) -> StateAccessResult<()> {
        // Coordinator-only operation
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot resume all workers".to_string(),
        ))
    }

    async fn add_message(
        &self,
        thread: &str,
        sender: &str,
        content: &str,
    ) -> StateAccessResult<i64> {
        Ok(HttpState::add_message(self, thread, sender, content, false).await?)
    }

    async fn get_messages(&self, thread: &str, limit: i64) -> StateAccessResult<Vec<Message>> {
        Ok(HttpState::get_messages(self, thread, limit).await?)
    }

    async fn get_unread_messages(
        &self,
        thread: &str,
        reader: &str,
    ) -> StateAccessResult<Vec<Message>> {
        Ok(HttpState::get_unread_messages(self, thread, reader).await?)
    }

    async fn get_all_unread_messages(&self, reader: &str) -> StateAccessResult<Vec<Message>> {
        Ok(HttpState::get_all_unread_messages(self, reader).await?)
    }

    async fn mark_messages_read(
        &self,
        thread: &str,
        reader: &str,
        up_to_id: Option<i64>,
    ) -> StateAccessResult<()> {
        Ok(HttpState::mark_messages_read(self, thread, reader, up_to_id).await?)
    }

    async fn get_threads(&self) -> StateAccessResult<Vec<String>> {
        Ok(HttpState::get_threads(self).await?)
    }

    async fn start_eval(
        &self,
        _branch: &str,
        _eval_name: Option<&str>,
        _log_file: Option<&str>,
    ) -> StateAccessResult<i64> {
        // Eval is managed by coordinator
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot start evals".to_string(),
        ))
    }

    async fn complete_eval(
        &self,
        _eval_id: i64,
        _success: bool,
        _feedback: &str,
    ) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot complete evals".to_string(),
        ))
    }

    async fn get_eval(&self, _eval_id: i64) -> StateAccessResult<Option<Eval>> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot get evals".to_string(),
        ))
    }

    async fn get_evals(&self, _limit: i64) -> StateAccessResult<Vec<Eval>> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot get evals".to_string(),
        ))
    }

    async fn get_running_eval(&self) -> StateAccessResult<Option<Eval>> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot get running eval".to_string(),
        ))
    }

    async fn cancel_running_evals(&self, _reason: &str) -> StateAccessResult<i64> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot cancel evals".to_string(),
        ))
    }

    async fn get_request(&self) -> StateAccessResult<Option<String>> {
        Ok(HttpState::get_request(self).await?)
    }

    async fn set_request(&self, _request: Option<&str>) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot set request".to_string(),
        ))
    }

    async fn get_project_path(&self) -> StateAccessResult<Option<String>> {
        Ok(HttpState::get_project_path(self).await?)
    }

    async fn set_project_path(&self, _path: &str) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot set project path".to_string(),
        ))
    }

    async fn get_waiting_reason(&self) -> StateAccessResult<Option<String>> {
        // Not implemented for remote workers yet
        Ok(None)
    }

    async fn set_waiting_reason(&self, reason: Option<&str>) -> StateAccessResult<()> {
        Ok(HttpState::set_waiting_reason(self, reason).await?)
    }

    async fn get_human_in_the_loop(&self) -> StateAccessResult<bool> {
        Ok(HttpState::get_human_in_the_loop(self).await?)
    }

    async fn set_human_in_the_loop(&self, _enabled: bool) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot set HITL mode".to_string(),
        ))
    }

    async fn get_summary(&self) -> StateAccessResult<Option<String>> {
        // Not implemented for remote workers yet
        Ok(None)
    }

    async fn set_summary(&self, _summary: &str) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot set summary".to_string(),
        ))
    }

    async fn get_worker_scale(&self) -> StateAccessResult<Option<String>> {
        // Not needed for workers
        Ok(None)
    }

    async fn set_worker_scale(&self, _scale: &str) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot set worker scale".to_string(),
        ))
    }

    async fn get_time_limit_minutes(&self) -> StateAccessResult<Option<i64>> {
        Ok(HttpState::get_time_limit_minutes(self).await?)
    }

    async fn set_time_limit_minutes(&self, _minutes: Option<i64>) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot set time limit".to_string(),
        ))
    }

    async fn get_started_at(&self) -> StateAccessResult<Option<String>> {
        // Not implemented for remote workers yet
        Ok(None)
    }

    async fn set_started_at(&self, _timestamp: Option<&str>) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot set started_at".to_string(),
        ))
    }

    async fn get_time_info(&self) -> StateAccessResult<Option<TimeInfo>> {
        Ok(HttpState::get_time_info(self).await?)
    }

    async fn is_time_expired(&self) -> StateAccessResult<bool> {
        Ok(HttpState::is_time_expired(self).await?)
    }

    async fn get_last_time_notification_pct(&self) -> StateAccessResult<Option<i64>> {
        // Not needed for workers
        Ok(None)
    }

    async fn set_last_time_notification_pct(&self, _pct: i64) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot set time notification pct".to_string(),
        ))
    }

    async fn clear_time_tracking(&self) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot clear time tracking".to_string(),
        ))
    }

    async fn get_iteration_count(&self) -> StateAccessResult<i64> {
        Ok(HttpState::get_iteration_count(self).await?)
    }

    async fn increment_iteration(&self) -> StateAccessResult<i64> {
        Ok(HttpState::increment_iteration(self).await?)
    }

    async fn get_max_iterations(&self) -> StateAccessResult<Option<i64>> {
        Ok(HttpState::get_max_iterations(self).await?)
    }

    async fn set_max_iterations(&self, _max_iter: Option<i64>) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot set max iterations".to_string(),
        ))
    }

    async fn get_history(&self, _limit: i64) -> StateAccessResult<Vec<HistoryEntry>> {
        // Not implemented for remote workers
        Ok(vec![])
    }

    async fn init_state(&self, _project_path: Option<&str>) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot initialize state".to_string(),
        ))
    }

    async fn heartbeat(&self) -> StateAccessResult<Status> {
        Ok(HttpState::heartbeat(self).await?)
    }
}
