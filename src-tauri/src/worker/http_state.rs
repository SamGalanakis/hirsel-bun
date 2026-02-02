//! HTTP client for remote workers connecting to coordinator API.
//!
//! This provides a StateAccess-like interface over HTTP for remote workers
//! that connect to the coordinator via SSH tunnel.

use std::time::Duration;

use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

use crate::core::http_client::ResponseExt;
use crate::core::state::{Message, Status, Worker, WorkerStatus};

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

/// Convert HttpError to HttpStateError
fn http_error_to_state_error(e: crate::core::http_client::HttpError) -> HttpStateError {
    match e {
        crate::core::http_client::HttpError::Response { status, body, .. } => {
            HttpStateError::Operation {
                status,
                message: body,
            }
        }
        crate::core::http_client::HttpError::Request(e) => HttpStateError::Connection(e),
        crate::core::http_client::HttpError::Parse(msg) => HttpStateError::InvalidResponse(msg),
    }
}

// =============================================================================
// HTTP State Client
// =============================================================================

/// HTTP client implementation for remote workers.
///
/// Connects to the coordinator's HTTP API via SSH tunnel.
pub struct HttpState {
    base_url: String,
    worker_name: String,
    run_name: String,
    client: Client,
}

impl HttpState {
    /// Create a new HTTP state client.
    pub fn new(base_url: &str, worker_name: &str, timeout_secs: u64) -> Self {
        // Get run_name from environment variable (set by hirsel when spawning workers)
        let run_name = std::env::var("HIRSEL_RUN").unwrap_or_default();

        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            worker_name: worker_name.to_string(),
            run_name,
            client,
        }
    }

    // =========================================================================
    // Internal HTTP methods
    // =========================================================================

    /// Build a run-scoped endpoint path
    fn run_endpoint(&self, path: &str) -> String {
        format!("/api/runs/{}{}", self.run_name, path)
    }

    async fn get<T: DeserializeOwned>(&self, endpoint: &str) -> HttpStateResult<T> {
        let url = format!("{}{}", self.base_url, endpoint);
        self.client
            .get(&url)
            .send()
            .await?
            .json_or_error()
            .await
            .map_err(http_error_to_state_error)
    }

    async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> HttpStateResult<T> {
        let url = format!("{}{}", self.base_url, endpoint);
        self.client
            .post(&url)
            .json(body)
            .send()
            .await?
            .json_or_error()
            .await
            .map_err(http_error_to_state_error)
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
    // Worker operations
    // =========================================================================

    pub async fn get_workers(&self) -> HttpStateResult<Vec<Worker>> {
        #[derive(Deserialize)]
        struct WorkersResponse {
            workers: Vec<Worker>,
        }
        let endpoint = self.run_endpoint("/workers/list");
        let result: WorkersResponse = self.get(&endpoint).await?;
        Ok(result.workers)
    }

    pub async fn get_worker(&self, name: &str) -> HttpStateResult<Option<Worker>> {
        #[derive(Deserialize)]
        struct WorkerResponse {
            worker: Option<Worker>,
        }
        let endpoint = self.run_endpoint(&format!("/workers/{}", name));
        let result: WorkerResponse = self.get(&endpoint).await?;
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
        let endpoint = self.run_endpoint(&format!("/workers/{}/update", name));
        let _: SuccessResponse = self
            .post(
                &endpoint,
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
        let endpoint = self.run_endpoint(&format!("/workers/{}/heartbeat", self.worker_name));
        let result: HeartbeatResponse = self.post(&endpoint, &Empty {}).await?;
        Status::from_str(&result.status).ok_or_else(|| {
            HttpStateError::InvalidResponse(format!("Invalid status: {}", result.status))
        })
    }

    pub async fn request_scaling_check(&self) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct Empty {}
        let endpoint = self.run_endpoint("/scaling_check");
        let _: SuccessResponse = self.post(&endpoint, &Empty {}).await?;
        Ok(())
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
    // Scribe operations
    // =========================================================================

    pub async fn add_scribe_submission(
        &self,
        worker_name: &str,
        content: &str,
    ) -> HttpStateResult<i64> {
        #[derive(Serialize)]
        struct ScribeRequest {
            worker_name: String,
            content: String,
        }
        #[derive(Deserialize)]
        struct ScribeResponse {
            id: i64,
        }
        let result: ScribeResponse = self
            .post(
                "/scribe",
                &ScribeRequest {
                    worker_name: worker_name.to_string(),
                    content: content.to_string(),
                },
            )
            .await?;
        Ok(result.id)
    }

    pub async fn read_docs(
        &self,
        file: Option<&str>,
    ) -> HttpStateResult<crate::core::files::DocsContent> {
        use crate::core::files::{DocFile, DocsContent};

        #[derive(Deserialize)]
        struct DocFileResponse {
            name: String,
            content: String,
        }
        #[derive(Deserialize)]
        struct DocsResponse {
            files: Vec<DocFileResponse>,
        }

        let endpoint = self.run_endpoint("/docs");
        let result: DocsResponse = self.get(&endpoint).await?;

        if let Some(filename) = file {
            // Return single file
            if let Some(doc) = result.files.into_iter().find(|f| f.name == filename) {
                Ok(DocsContent::Single {
                    name: doc.name,
                    content: doc.content,
                })
            } else {
                Ok(DocsContent::Single {
                    name: filename.to_string(),
                    content: String::new(),
                })
            }
        } else {
            // Return all files
            Ok(DocsContent::All {
                files: result
                    .files
                    .into_iter()
                    .map(|f| DocFile {
                        name: f.name,
                        content: f.content,
                    })
                    .collect(),
            })
        }
    }

    // =========================================================================
    // Config operations
    // =========================================================================

    pub async fn get_request(&self) -> HttpStateResult<Option<String>> {
        #[derive(Deserialize)]
        struct RequestResponse {
            request: Option<String>,
        }
        let endpoint = self.run_endpoint("/config/request");
        let result: RequestResponse = self.get(&endpoint).await?;
        Ok(result.request)
    }

    pub async fn get_project_path(&self) -> HttpStateResult<Option<String>> {
        #[derive(Deserialize)]
        struct PathResponse {
            project_path: Option<String>,
        }
        let endpoint = self.run_endpoint("/config/project_path");
        let result: PathResponse = self.get(&endpoint).await?;
        Ok(result.project_path)
    }

    pub async fn get_human_in_the_loop(&self) -> HttpStateResult<bool> {
        #[derive(Deserialize)]
        struct HitlResponse {
            enabled: bool,
        }
        let endpoint = self.run_endpoint("/config/human_in_the_loop");
        let result: HitlResponse = self.get(&endpoint).await?;
        Ok(result.enabled)
    }

    pub async fn set_waiting_reason(&self, reason: Option<&str>) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct ReasonRequest {
            reason: Option<String>,
        }
        let endpoint = self.run_endpoint("/config/waiting_reason");
        let _: SuccessResponse = self
            .post(
                &endpoint,
                &ReasonRequest {
                    reason: reason.map(|s| s.to_string()),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn get_project_id(&self) -> HttpStateResult<Option<i64>> {
        #[derive(Deserialize)]
        struct ProjectIdResponse {
            project_id: Option<i64>,
        }
        let endpoint = self.run_endpoint("/config/project_id");
        let result: ProjectIdResponse = self.get(&endpoint).await?;
        Ok(result.project_id)
    }

    // =========================================================================
    // Board Integration (Live Nodes)
    // =========================================================================

    pub async fn add_live_node(
        &self,
        id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<Vec<&str>>,
        node_type: &str,
        content: &str,
    ) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct AddLiveNodeRequest {
            id: String,
            name: String,
            parent_id: Option<String>,
            blocked_by: Option<Vec<String>>,
            node_type: String,
            content: String,
        }
        let endpoint = self.run_endpoint("/live-nodes");
        let _: SuccessResponse = self
            .post(
                &endpoint,
                &AddLiveNodeRequest {
                    id: id.to_string(),
                    name: name.to_string(),
                    parent_id: parent_id.map(|s| s.to_string()),
                    blocked_by: blocked_by.map(|b| b.iter().map(|s| s.to_string()).collect()),
                    node_type: node_type.to_string(),
                    content: content.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn claim_live_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> HttpStateResult<crate::core::delta::LiveNode> {
        #[derive(Serialize)]
        struct ClaimRequest {
            worker_name: String,
        }
        let endpoint = self.run_endpoint(&format!("/live-nodes/{}/claim", id));
        self.post(
            &endpoint,
            &ClaimRequest {
                worker_name: worker_name.to_string(),
            },
        )
        .await
    }

    pub async fn complete_live_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> HttpStateResult<crate::core::delta::LiveNode> {
        #[derive(Serialize)]
        struct CompleteRequest {
            worker_name: String,
        }
        let endpoint = self.run_endpoint(&format!("/live-nodes/{}/complete", id));
        self.post(
            &endpoint,
            &CompleteRequest {
                worker_name: worker_name.to_string(),
            },
        )
        .await
    }

    pub async fn unclaim_live_node(&self, id: &str) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct Empty {}
        let endpoint = self.run_endpoint(&format!("/live-nodes/{}/unclaim", id));
        let _: SuccessResponse = self.post(&endpoint, &Empty {}).await?;
        Ok(())
    }

    pub async fn get_claimable_live_nodes(
        &self,
    ) -> HttpStateResult<Vec<crate::core::delta::LiveNode>> {
        #[derive(Deserialize)]
        struct NodesResponse {
            nodes: Vec<crate::core::delta::LiveNode>,
        }
        let endpoint = self.run_endpoint("/live-nodes/claimable");
        let result: NodesResponse = self.get(&endpoint).await?;
        Ok(result.nodes)
    }

    pub async fn get_claimed_live_node(
        &self,
        worker_name: &str,
    ) -> HttpStateResult<Option<crate::core::delta::LiveNode>> {
        #[derive(Deserialize)]
        struct NodeResponse {
            node: Option<crate::core::delta::LiveNode>,
        }
        let endpoint = self.run_endpoint(&format!("/workers/{}/claimed-node", worker_name));
        let result: NodeResponse = self.get(&endpoint).await?;
        Ok(result.node)
    }

    pub async fn get_live_nodes(&self) -> HttpStateResult<Vec<crate::core::delta::LiveNode>> {
        #[derive(Deserialize)]
        struct NodesResponse {
            nodes: Vec<crate::core::delta::LiveNode>,
        }
        let endpoint = self.run_endpoint("/live-nodes");
        let result: NodesResponse = self.get(&endpoint).await?;
        Ok(result.nodes)
    }

    pub async fn is_live_node_blocked(&self, id: &str) -> HttpStateResult<bool> {
        #[derive(Deserialize)]
        struct BlockedResponse {
            blocked: bool,
        }
        let endpoint = self.run_endpoint(&format!("/live-nodes/{}/blocked", id));
        let result: BlockedResponse = self.get(&endpoint).await?;
        Ok(result.blocked)
    }

    pub async fn live_node_eval_pass(
        &self,
        eval_id: &str,
        worker_name: &str,
    ) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct EvalPassRequest {
            worker_name: String,
        }
        let endpoint = self.run_endpoint(&format!("/live-nodes/{}/eval-pass", eval_id));
        let _: SuccessResponse = self
            .post(
                &endpoint,
                &EvalPassRequest {
                    worker_name: worker_name.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn live_node_eval_fail(
        &self,
        eval_id: &str,
        worker_name: &str,
        feedback: &str,
    ) -> HttpStateResult<String> {
        #[derive(Serialize)]
        struct EvalFailRequest {
            worker_name: String,
            feedback: String,
        }
        #[derive(Deserialize)]
        struct EvalFailResponse {
            repair_node_id: String,
        }
        let endpoint = self.run_endpoint(&format!("/live-nodes/{}/eval-fail", eval_id));
        let result: EvalFailResponse = self
            .post(
                &endpoint,
                &EvalFailRequest {
                    worker_name: worker_name.to_string(),
                    feedback: feedback.to_string(),
                },
            )
            .await?;
        Ok(result.repair_node_id)
    }

    pub async fn set_live_node_tokens(&self, id: &str, tokens: i64) -> HttpStateResult<()> {
        #[derive(Serialize)]
        struct TokensRequest {
            tokens: i64,
        }
        let endpoint = self.run_endpoint(&format!("/live-nodes/{}/tokens", id));
        let _: SuccessResponse = self.post(&endpoint, &TokensRequest { tokens }).await?;
        Ok(())
    }

    pub async fn get_validated_nodes(&self, eval_id: &str) -> HttpStateResult<Vec<String>> {
        #[derive(Deserialize)]
        struct ValidatedNodesResponse {
            node_ids: Vec<String>,
        }
        let endpoint = self.run_endpoint(&format!("/live-nodes/{}/validated", eval_id));
        let result: ValidatedNodesResponse = self.get(&endpoint).await?;
        Ok(result.node_ids)
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

    pub async fn get_active_workers(&self) -> HttpStateResult<Vec<Worker>> {
        #[derive(Deserialize)]
        struct WorkersResponse {
            workers: Vec<Worker>,
        }
        let endpoint = self.run_endpoint("/workers/active");
        let result: WorkersResponse = self.get(&endpoint).await?;
        Ok(result.workers)
    }

    pub async fn all_workers_done(&self) -> HttpStateResult<bool> {
        #[derive(Deserialize)]
        struct DoneResponse {
            all_done: bool,
        }
        let endpoint = self.run_endpoint("/workers/all_done");
        let result: DoneResponse = self.get(&endpoint).await?;
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

    async fn get_history(&self, _limit: i64) -> StateAccessResult<Vec<HistoryEntry>> {
        // Not implemented for remote workers
        Ok(vec![])
    }

    async fn add_scribe_submission(
        &self,
        worker_name: &str,
        content: &str,
    ) -> StateAccessResult<i64> {
        Ok(HttpState::add_scribe_submission(self, worker_name, content).await?)
    }

    async fn read_docs(
        &self,
        file: Option<&str>,
    ) -> StateAccessResult<crate::core::files::DocsContent> {
        Ok(HttpState::read_docs(self, file).await?)
    }

    async fn init_state(&self, _project_path: Option<&str>) -> StateAccessResult<()> {
        Err(StateAccessError::InvalidOperation(
            "Remote workers cannot initialize state".to_string(),
        ))
    }

    async fn heartbeat(&self) -> StateAccessResult<Status> {
        Ok(HttpState::heartbeat(self).await?)
    }

    async fn request_scaling_check(&self) -> StateAccessResult<()> {
        // Remote workers trigger scaling via the coordinator
        // The scaling check will be processed by the daemon on the coordinator side
        Ok(HttpState::request_scaling_check(self).await?)
    }

    async fn get_project_id(&self) -> StateAccessResult<Option<i64>> {
        Ok(HttpState::get_project_id(self).await?)
    }

    async fn add_live_node(
        &self,
        id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
        node_type: &str,
        content: &str,
    ) -> StateAccessResult<()> {
        let blocked_by_vec = blocked_by.map(|b| b.to_vec());
        Ok(HttpState::add_live_node(
            self,
            id,
            name,
            parent_id,
            blocked_by_vec,
            node_type,
            content,
        )
        .await?)
    }

    async fn claim_live_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> StateAccessResult<crate::core::delta::LiveNode> {
        Ok(HttpState::claim_live_node(self, id, worker_name).await?)
    }

    async fn complete_live_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> StateAccessResult<crate::core::delta::LiveNode> {
        Ok(HttpState::complete_live_node(self, id, worker_name).await?)
    }

    async fn unclaim_live_node(&self, id: &str) -> StateAccessResult<()> {
        Ok(HttpState::unclaim_live_node(self, id).await?)
    }

    async fn get_claimed_live_node(
        &self,
        worker_name: &str,
    ) -> StateAccessResult<Option<crate::core::delta::LiveNode>> {
        Ok(HttpState::get_claimed_live_node(self, worker_name).await?)
    }

    async fn get_claimable_nodes(&self) -> StateAccessResult<Vec<crate::core::delta::LiveNode>> {
        Ok(HttpState::get_claimable_live_nodes(self).await?)
    }

    async fn get_live_nodes(&self) -> StateAccessResult<Vec<crate::core::delta::LiveNode>> {
        Ok(HttpState::get_live_nodes(self).await?)
    }

    async fn is_live_node_blocked(&self, id: &str) -> StateAccessResult<bool> {
        Ok(HttpState::is_live_node_blocked(self, id).await?)
    }

    async fn live_node_eval_pass(&self, eval_id: &str, worker_name: &str) -> StateAccessResult<()> {
        Ok(HttpState::live_node_eval_pass(self, eval_id, worker_name).await?)
    }

    async fn live_node_eval_fail(
        &self,
        eval_id: &str,
        worker_name: &str,
        feedback: &str,
    ) -> StateAccessResult<String> {
        Ok(HttpState::live_node_eval_fail(self, eval_id, worker_name, feedback).await?)
    }

    async fn set_live_node_tokens(&self, id: &str, tokens: i64) -> StateAccessResult<()> {
        Ok(HttpState::set_live_node_tokens(self, id, tokens).await?)
    }

    async fn get_validated_nodes(&self, eval_id: &str) -> StateAccessResult<Vec<String>> {
        Ok(HttpState::get_validated_nodes(self, eval_id).await?)
    }
}
