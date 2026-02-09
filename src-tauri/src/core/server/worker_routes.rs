//! Shared worker HTTP route handlers.
//!
//! These functions implement the core logic for worker API endpoints,
//! used by both daemon (multi-run) and coordinator_api (single-run).
//!
//! Each function takes:
//! - `&SQLiteState` for state access
//! - Optional `&LocalLifecycleManager` for lifecycle handling
//!
//! The HTTP layer (daemon/coordinator_api) is responsible for:
//! - Extracting state from headers or shared state
//! - Creating the lifecycle manager
//! - Converting results to HTTP responses

use serde::{Deserialize, Serialize};

use crate::core::lifecycle::{LifecycleEvent, LifecycleManager, LocalLifecycleManager};
use crate::core::state::{SQLiteState, StateResult, Status, Worker, WorkerStatus, WorkerUpdate};

// =============================================================================
// Response Types
// =============================================================================

#[derive(Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SuccessResponse {
    pub fn ok() -> Self {
        Self {
            success: true,
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            error: Some(msg.into()),
        }
    }
}

#[derive(Serialize)]
pub struct WorkersResponse {
    pub workers: Vec<Worker>,
}

#[derive(Serialize)]
pub struct WorkerResponse {
    pub worker: Option<Worker>,
}

#[derive(Serialize)]
pub struct AllDoneResponse {
    pub all_done: bool,
}

#[derive(Serialize)]
pub struct HeartbeatResponse {
    pub status: String,
}

// =============================================================================
// Request Types
// =============================================================================

#[derive(Deserialize)]
pub struct UpdateWorkerRequest {
    #[serde(default)]
    pub pid: Option<i64>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub waiting_thread: Option<String>,
    #[serde(default)]
    pub needs_restart: Option<bool>,
    #[serde(default)]
    pub last_heartbeat: Option<String>,
}

impl UpdateWorkerRequest {
    /// Parse status string to WorkerStatus
    pub fn parse_status(&self) -> Option<WorkerStatus> {
        self.status.as_ref().and_then(|s| match s.as_str() {
            "working" => Some(WorkerStatus::Working),
            "awaiting" => Some(WorkerStatus::Awaiting),
            "paused" => Some(WorkerStatus::Paused),
            "error" => Some(WorkerStatus::Error),
            _ => None,
        })
    }

    /// Convert to WorkerUpdate struct
    pub fn to_worker_update(&self) -> WorkerUpdate {
        WorkerUpdate {
            pid: self.pid.map(Some),
            session_id: self.session_id.clone(),
            status: self.parse_status(),
            waiting_thread: self.waiting_thread.clone(),
            needs_restart: self.needs_restart,
            last_heartbeat: self.last_heartbeat.clone(),
            ..Default::default()
        }
    }
}

#[derive(Deserialize)]
pub struct CreateWorkerRequest {
    pub name: String,
    pub work_dir: String,
    pub location: String,
}

#[derive(Deserialize)]
pub struct ReasonRequest {
    pub reason: String,
}

// =============================================================================
// Handler Functions
// =============================================================================

/// List all workers
pub async fn list_workers(state: &SQLiteState) -> StateResult<Vec<Worker>> {
    state.get_workers().await
}

/// List active workers (working status)
pub async fn list_active_workers(state: &SQLiteState) -> StateResult<Vec<Worker>> {
    state.get_active_workers().await
}

/// Check if all workers are done (inactive)
pub async fn all_workers_done(state: &SQLiteState) -> StateResult<bool> {
    state.all_workers_inactive().await
}

/// Get a specific worker
pub async fn get_worker(state: &SQLiteState, name: &str) -> StateResult<Option<Worker>> {
    state.get_worker(name).await
}

/// Update worker state with optional lifecycle handling
pub async fn update_worker(
    state: &SQLiteState,
    name: &str,
    request: &UpdateWorkerRequest,
    lifecycle: Option<&LocalLifecycleManager>,
) -> StateResult<()> {
    // Get old status for lifecycle event
    let old_status = state.get_worker(name).await?.map(|w| w.status);
    let new_status = request.parse_status();

    // Apply update
    let update = request.to_worker_update();
    state.update_worker(name, update).await?;

    // Trigger lifecycle event if status changed to Awaiting
    if let (Some(lifecycle), Some(old), Some(new)) = (lifecycle, old_status, new_status) {
        if old != new && new == WorkerStatus::Awaiting {
            let _ = lifecycle
                .process_event(LifecycleEvent::WorkerStatusChanged {
                    worker_name: name.to_string(),
                    old,
                    new,
                })
                .await;
        }
    }

    Ok(())
}

/// Update worker heartbeat and return current run status
pub async fn worker_heartbeat(state: &SQLiteState, name: &str) -> StateResult<Status> {
    let now = chrono::Utc::now().to_rfc3339();
    let update = WorkerUpdate {
        last_heartbeat: Some(now),
        ..Default::default()
    };
    state.update_worker(name, update).await?;
    state.status().await
}

/// Create a new worker
pub async fn create_worker(
    state: &SQLiteState,
    name: &str,
    work_dir: &str,
    location: &str,
) -> StateResult<Option<Worker>> {
    state.add_worker(name, work_dir, location).await
}

/// Pause all workers
pub async fn pause_all_workers(state: &SQLiteState, reason: &str) -> StateResult<()> {
    state.pause_all_workers(reason).await
}

/// Resume all workers
pub async fn resume_all_workers(state: &SQLiteState) -> StateResult<()> {
    state.resume_all_workers().await
}

/// Request a scaling check
pub async fn request_scaling_check(state: &SQLiteState) -> StateResult<()> {
    state.request_scaling_check().await
}
