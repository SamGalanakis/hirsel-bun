//! API route handlers

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use lash::oauth;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::AppState;
use crate::core::api_types::{
    ConfigResponse, Eval, HistoryEntry, RunDetail, RunSummary, Worker, WorkerEventsResponse,
};
use crate::core::orchestrator::{
    DeliverRunRequest, HealthResponse, Orchestrator, OrchestratorError, ResumeRunRequest,
    ResumeWorkerRequest, SpawnSingleWorkerRequest,
};

/// Convert OrchestratorError to HTTP response
impl IntoResponse for OrchestratorError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            OrchestratorError::RunNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            OrchestratorError::WorkerNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            OrchestratorError::InvalidOperation(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        (status, Json(ErrorResponse { error: message })).into_response()
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

type Result<T> = std::result::Result<T, OrchestratorError>;

// =============================================================================
// Health
// =============================================================================

pub async fn health(State(state): State<Arc<AppState>>) -> Result<Json<HealthResponse>> {
    let health = state.orchestrator.health().await?;
    Ok(Json(health))
}

// =============================================================================
// Run Management
// =============================================================================

pub async fn list_runs(State(state): State<Arc<AppState>>) -> Result<Json<Vec<RunSummary>>> {
    let runs = state.orchestrator.list_runs().await?;
    Ok(Json(runs))
}

pub async fn get_run(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<RunDetail>> {
    let run = state.orchestrator.get_run(&name).await?;
    Ok(Json(run))
}

pub async fn delete_run(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<StatusCode> {
    state.orchestrator.delete_run(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn pause_run(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<StatusCode> {
    state.orchestrator.pause_run(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn resume_run(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<ResumeRunRequest>,
) -> Result<StatusCode> {
    state
        .orchestrator
        .resume_run(&name, body.time_limit_minutes)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct DeliverResponse {
    branch: String,
}

pub async fn deliver_run(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<DeliverRunRequest>,
) -> Result<Json<DeliverResponse>> {
    let branch = state.orchestrator.deliver_run(&name, body.branch).await?;
    Ok(Json(DeliverResponse { branch }))
}

// =============================================================================
// File Transfer
// =============================================================================

/// Initialize or reinitialize workspace for a run
///
/// This allows workspace setup to be done separately from run creation.
/// Useful for:
/// - Initializing workspace for a run created without a starting_point
/// - Reinitializing workspace (e.g., to switch to a different branch)
pub async fn init_workspace(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<crate::core::orchestrator::InitWorkspaceRequest>,
) -> Result<Json<crate::core::orchestrator::InitWorkspaceResponse>> {
    let response = state.orchestrator.init_workspace(&name, body).await?;
    Ok(Json(response))
}

/// Download working directory as tarball
///
/// Returns a gzipped tar archive of the run's work directory.
/// Workers call this to get project files.
///
/// For local runs (which create work/staging/), serves from staging.
/// For remote uploads (which go to work/ directly), serves from work.
pub async fn download_files(
    State(_state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> std::result::Result<impl IntoResponse, OrchestratorError> {
    use crate::core::config;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use tar::Builder;

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let work_dir = runtime_dir.join("work");
    if !work_dir.exists() {
        return Err(OrchestratorError::Other(
            "Work directory not found. Upload files first.".into(),
        ));
    }

    // Determine source directory:
    // - Local runs create work/staging/ with project files
    // - Remote uploads go directly to work/
    // Prefer staging if it exists (local run with remote runners)
    let staging_dir = work_dir.join("staging");
    let source_dir = if staging_dir.exists() {
        staging_dir
    } else {
        work_dir
    };

    tracing::debug!(
        "Serving files tarball from {} for run '{}'",
        source_dir.display(),
        name
    );

    // Create tarball
    let mut buffer = Vec::new();
    {
        let encoder = GzEncoder::new(&mut buffer, Compression::fast());
        let mut builder = Builder::new(encoder);

        builder
            .append_dir_all(".", &source_dir)
            .map_err(|e| OrchestratorError::Other(format!("Failed to create tarball: {}", e)))?;

        builder
            .into_inner()
            .map_err(|e| OrchestratorError::Other(format!("Failed to finish tarball: {}", e)))?
            .finish()
            .map_err(|e| OrchestratorError::Other(format!("Failed to compress: {}", e)))?;
    }

    tracing::info!("Serving files for run '{}' ({} bytes)", name, buffer.len());

    Ok(([(header::CONTENT_TYPE, "application/gzip")], buffer))
}

/// Start a run (unified entry point for CLI and GUI)
///
/// This creates the run directory, workspace, and optionally spawns workers.
pub async fn start_run(
    State(state): State<Arc<AppState>>,
    Json(body): Json<crate::core::orchestrator::StartRunRequest>,
) -> Result<Json<RunDetail>> {
    let detail = state.orchestrator.start_run(body).await?;
    Ok(Json(detail))
}

// =============================================================================
// Workers
// =============================================================================

pub async fn list_workers(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<Vec<Worker>>> {
    let workers = state.orchestrator.list_workers(&name).await?;
    Ok(Json(workers))
}

pub async fn restart_worker(
    State(state): State<Arc<AppState>>,
    Path((name, worker)): Path<(String, String)>,
) -> Result<StatusCode> {
    state.orchestrator.restart_worker(&name, &worker).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Spawn a single worker (used by daemon for lifecycle management)
pub async fn spawn_single_worker(
    State(state): State<Arc<AppState>>,
    Path((name, worker)): Path<(String, String)>,
    Json(body): Json<SpawnSingleWorkerRequest>,
) -> Result<StatusCode> {
    let work_dir = std::path::PathBuf::from(&body.work_dir);
    state
        .orchestrator
        .spawn_single_worker(&name, &worker, &work_dir, body.resume_session_id.as_deref())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Resume a worker with optional snapshot/session restoration
pub async fn resume_worker(
    State(state): State<Arc<AppState>>,
    Path((name, worker)): Path<(String, String)>,
    Json(body): Json<ResumeWorkerRequest>,
) -> Result<StatusCode> {
    let work_dir = std::path::PathBuf::from(&body.work_dir);
    state
        .orchestrator
        .resume_worker(
            &name,
            &worker,
            &work_dir,
            body.resume_session_id.as_deref(),
            body.state_handle.as_ref(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct WorkerEventParams {
    after_id: Option<i64>,
    limit: Option<i64>,
}

pub async fn get_worker_events(
    State(state): State<Arc<AppState>>,
    Path((name, worker)): Path<(String, String)>,
    Query(params): Query<WorkerEventParams>,
) -> Result<Json<WorkerEventsResponse>> {
    let events = state
        .orchestrator
        .get_worker_events(&name, &worker, params.after_id, params.limit)
        .await?;
    Ok(Json(events))
}

#[derive(Debug, Deserialize)]
pub struct ScribeSubmitRequest {
    pub worker_name: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ScribeSubmitResponse {
    pub id: i64,
}

pub async fn add_scribe(
    Path(name): Path<String>,
    Json(body): Json<ScribeSubmitRequest>,
) -> Result<Json<ScribeSubmitResponse>> {
    use crate::core::state::SQLiteState;

    let state = SQLiteState::new(&name).await?;
    let id = state
        .add_scribe_submission(&body.worker_name, &body.content)
        .await?;

    Ok(Json(ScribeSubmitResponse { id }))
}

#[derive(Debug, Serialize)]
pub struct RetainedContextResponse {
    pub markdown: String,
}

pub async fn get_retained_context(
    Path(name): Path<String>,
) -> Result<Json<RetainedContextResponse>> {
    use crate::core::project::ProjectStore;
    use crate::core::state::SQLiteState;

    let state = SQLiteState::new(&name).await?;
    let project_id = state.get_project_id().await?.ok_or_else(|| {
        OrchestratorError::InvalidOperation("Run is not linked to a project".to_string())
    })?;
    let store = ProjectStore::open()
        .await
        .map_err(|e| OrchestratorError::Other(e.to_string()))?;
    let context = store
        .get_project_retained_context(project_id)
        .await
        .map_err(|e| OrchestratorError::Other(e.to_string()))?;

    Ok(Json(RetainedContextResponse {
        markdown: context.markdown,
    }))
}

// =============================================================================
// Evals
// =============================================================================

pub async fn list_evals(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<Vec<Eval>>> {
    let evals = state.orchestrator.list_evals(&name).await?;
    Ok(Json(evals))
}

// =============================================================================
// History
// =============================================================================

#[derive(Deserialize)]
pub struct HistoryParams {
    limit: Option<u32>,
}

pub async fn get_history(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<Vec<HistoryEntry>>> {
    let history = state.orchestrator.get_history(&name, params.limit).await?;
    Ok(Json(history))
}

// =============================================================================
// Config
// =============================================================================

pub async fn get_config(State(state): State<Arc<AppState>>) -> Result<Json<ConfigResponse>> {
    let config = state.orchestrator.get_config().await?;
    Ok(Json(config))
}

// =============================================================================
// Config - Granular Updates
// =============================================================================

use crate::core::api_types::{
    mask_credential, CredentialStatusResponse, LlmConfigRequest, StoreCredentialRequest,
};
use crate::core::config::BackendConfig;
use crate::core::credentials::CredentialStore;

/// Patch LLM configuration
pub async fn patch_llm_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LlmConfigRequest>,
) -> Result<StatusCode> {
    let mut config = state.config.write().await;
    body.apply(&mut config.llm);
    config
        .save()
        .map_err(|e| OrchestratorError::Config(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Credentials
// =============================================================================

/// Store a credential
pub async fn store_credential(
    Path(key): Path<String>,
    Json(body): Json<StoreCredentialRequest>,
) -> Result<StatusCode> {
    let store = CredentialStore::open()
        .await
        .map_err(|e| OrchestratorError::Other(format!("Failed to open credential store: {}", e)))?;

    store
        .store(&key, &body.value)
        .await
        .map_err(|e| OrchestratorError::Other(format!("Failed to store credential: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Get credential status (masked value)
pub async fn get_credential(Path(key): Path<String>) -> Result<Json<CredentialStatusResponse>> {
    let store = CredentialStore::open()
        .await
        .map_err(|e| OrchestratorError::Other(format!("Failed to open credential store: {}", e)))?;

    match store.load(&key).await {
        Ok(value) => Ok(Json(CredentialStatusResponse {
            key: key.clone(),
            exists: true,
            masked_value: Some(mask_credential(&value)),
        })),
        Err(_) => Ok(Json(CredentialStatusResponse {
            key,
            exists: false,
            masked_value: None,
        })),
    }
}

/// Delete a credential
pub async fn delete_credential(Path(key): Path<String>) -> Result<StatusCode> {
    let store = CredentialStore::open()
        .await
        .map_err(|e| OrchestratorError::Other(format!("Failed to open credential store: {}", e)))?;

    store
        .delete(&key)
        .await
        .map_err(|e| OrchestratorError::Other(format!("Failed to delete credential: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Codex Device OAuth
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDeviceStartResponse {
    pub device_auth_id: String,
    pub user_code: String,
    pub verify_url: String,
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDevicePollRequest {
    pub device_auth_id: String,
    pub user_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDevicePollResponse {
    pub status: String,
    pub authorization_code: Option<String>,
    pub code_verifier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDeviceExchangeRequest {
    pub authorization_code: String,
    pub code_verifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDeviceExchangeResponse {
    pub status: String,
    pub expires_at: u64,
}

/// Start Codex device-code OAuth flow.
pub async fn codex_device_start() -> Result<Json<CodexDeviceStartResponse>> {
    let device = oauth::codex_request_device_code().await.map_err(|e| {
        OrchestratorError::Other(format!("Failed to start Codex device auth: {}", e))
    })?;

    Ok(Json(CodexDeviceStartResponse {
        device_auth_id: device.device_auth_id,
        user_code: device.user_code,
        verify_url: oauth::CODEX_DEVICE_VERIFY_URL.to_string(),
        interval: device.interval,
    }))
}

/// Poll Codex device authorization status.
pub async fn codex_device_poll(
    Json(body): Json<CodexDevicePollRequest>,
) -> Result<Json<CodexDevicePollResponse>> {
    let polled = oauth::codex_poll_device_auth(&body.device_auth_id, &body.user_code)
        .await
        .map_err(|e| {
            OrchestratorError::Other(format!("Failed to poll Codex device auth: {}", e))
        })?;

    match polled {
        Some((authorization_code, code_verifier)) => Ok(Json(CodexDevicePollResponse {
            status: "approved".to_string(),
            authorization_code: Some(authorization_code),
            code_verifier: Some(code_verifier),
        })),
        None => Ok(Json(CodexDevicePollResponse {
            status: "pending".to_string(),
            authorization_code: None,
            code_verifier: None,
        })),
    }
}

/// Exchange Codex device authorization code for tokens and persist credentials.
pub async fn codex_device_exchange(
    Json(body): Json<CodexDeviceExchangeRequest>,
) -> Result<Json<CodexDeviceExchangeResponse>> {
    let tokens = oauth::codex_exchange_code(&body.authorization_code, &body.code_verifier)
        .await
        .map_err(|e| {
            OrchestratorError::Other(format!("Failed to exchange Codex auth code: {}", e))
        })?;
    let expires_at = tokens.expires_at;

    let store = CredentialStore::open()
        .await
        .map_err(|e| OrchestratorError::Other(format!("Failed to open credential store: {}", e)))?;

    store
        .store_codex_oauth(&crate::core::credentials::CodexOAuthCredentials {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_at,
            account_id: tokens.account_id,
        })
        .await
        .map_err(|e| OrchestratorError::Other(format!("Failed to store Codex tokens: {}", e)))?;

    Ok(Json(CodexDeviceExchangeResponse {
        status: "ok".to_string(),
        expires_at,
    }))
}

// =============================================================================
// Config - Full Replace/Merge
// =============================================================================

/// Request body for PUT /api/config (full config replacement)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutConfigRequest {
    #[serde(default)]
    pub backend: Option<BackendConfig>,
    #[serde(default)]
    pub llm: Option<crate::core::config::LlmConfig>,
}

/// Replace entire config (PUT /api/config)
///
/// Replaces all provided fields in the config. Fields not provided are left unchanged.
/// Saves to both file and database.
pub async fn put_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PutConfigRequest>,
) -> Result<StatusCode> {
    let mut config = state.config.write().await;

    // Apply all provided fields
    if let Some(backend) = body.backend {
        config.backend = backend;
    }
    if let Some(llm) = body.llm {
        config.llm = llm;
    }

    // Save to database (primary) and file
    config
        .save()
        .map_err(|e| OrchestratorError::Config(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Merge partial config (PATCH /api/config)
///
/// Merges the provided partial config into the existing config.
/// Only provided fields are updated. Saves to both file and database.
pub async fn patch_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PutConfigRequest>,
) -> Result<StatusCode> {
    let mut config = state.config.write().await;

    if let Some(backend) = body.backend {
        config.backend = backend;
    }
    if let Some(llm) = body.llm {
        config.llm = llm;
    }

    // Save to database (primary) and file
    config
        .save()
        .map_err(|e| OrchestratorError::Config(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Board Integration - Nodes
// =============================================================================

/// Response for project_id endpoint
#[derive(Debug, Serialize)]
pub struct ProjectIdResponse {
    pub project_id: Option<i64>,
}

/// Get the project_id for a board-linked run
///
/// GET /api/runtimes/{name}/config/project_id
///
/// Workers use this to determine which project they're working on
/// so they can add nodes to the correct project.
pub async fn get_project_id(Path(name): Path<String>) -> Result<Json<ProjectIdResponse>> {
    use crate::core::{config, state::SQLiteState};

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let state = SQLiteState::new(&name).await?;

    let project_id = state.get_project_id().await?;

    Ok(Json(ProjectIdResponse { project_id }))
}

/// Request body for adding a node from a worker
#[derive(Debug, Deserialize)]
pub struct AddNodeRequest {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub blocked_by: Option<Vec<String>>,
    pub kind: String,
    pub content: String,
    pub validates: Option<Vec<String>>,
}

/// Add a node from a worker
///
/// POST /api/runtimes/{name}/nodes
///
/// Workers call this to add tasks to the board tree during execution.
/// The node is created with source='worker'.
pub async fn add_node(
    Path(name): Path<String>,
    Json(body): Json<AddNodeRequest>,
) -> Result<StatusCode> {
    use crate::core::delta::{DeltaState, NodeKind};
    use crate::core::{config, state::SQLiteState};

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let state = SQLiteState::new(&name).await?;

    // Get project_id and route_id for this run
    let project_id = state.get_project_id().await?.ok_or_else(|| {
        OrchestratorError::InvalidOperation("Run is not linked to a project".into())
    })?;
    let route_id = state
        .get_route_id()
        .await
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    // Parse node kind (defaults to Task for unknown kinds)
    let kind = NodeKind::from_str(&body.kind);

    // Create node via DeltaState
    let delta_state = DeltaState::with_route(project_id, route_id);

    let blocked_by: Option<Vec<&str>> = body
        .blocked_by
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect());

    let validates: Option<Vec<&str>> = body
        .validates
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect());

    delta_state
        .create_node_from_worker(
            &body.id,
            &body.name,
            body.parent_id.as_deref(),
            blocked_by.as_deref(),
            kind,
            &body.content,
            validates.as_deref(),
        )
        .await?;

    Ok(StatusCode::CREATED)
}

/// Get all nodes for a run
///
/// GET /api/runtimes/{name}/nodes
pub async fn get_nodes(Path(name): Path<String>) -> Result<Json<NodesResponse>> {
    use crate::core::delta::DeltaState;
    use crate::core::{config, state::SQLiteState};

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let state = SQLiteState::new(&name).await?;

    let project_id = state.get_project_id().await?.ok_or_else(|| {
        OrchestratorError::InvalidOperation("Run is not linked to a project".into())
    })?;
    let route_id = state
        .get_route_id()
        .await
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    let delta_state = DeltaState::with_route(project_id, route_id);
    let nodes = delta_state.get_nodes().await?;

    Ok(Json(NodesResponse { nodes }))
}

#[derive(Debug, Serialize)]
pub struct NodesResponse {
    pub nodes: Vec<crate::core::delta::BoardNode>,
}

/// Get claimable nodes for a run
///
/// GET /api/runtimes/{name}/nodes/claimable
pub async fn get_claimable_nodes(Path(name): Path<String>) -> Result<Json<NodesResponse>> {
    use crate::core::delta::DeltaState;
    use crate::core::{config, state::SQLiteState};

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let state = SQLiteState::new(&name).await?;

    let project_id = state.get_project_id().await?.ok_or_else(|| {
        OrchestratorError::InvalidOperation("Run is not linked to a project".into())
    })?;
    let route_id = state
        .get_route_id()
        .await
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    let delta_state = DeltaState::with_route(project_id, route_id);
    let nodes = delta_state.get_claimable_nodes().await?;

    Ok(Json(NodesResponse { nodes }))
}

#[derive(Debug, Deserialize)]
pub struct ClaimNodeRequest {
    pub worker_name: String,
}

/// Claim a node for a worker
///
/// POST /api/runtimes/{name}/nodes/{id}/claim
pub async fn claim_node(
    Path((name, node_id)): Path<(String, String)>,
    Json(body): Json<ClaimNodeRequest>,
) -> Result<Json<crate::core::delta::BoardNode>> {
    use crate::core::delta::DeltaState;
    use crate::core::{config, state::SQLiteState};

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let state = SQLiteState::new(&name).await?;

    let project_id = state.get_project_id().await?.ok_or_else(|| {
        OrchestratorError::InvalidOperation("Run is not linked to a project".into())
    })?;
    let route_id = state
        .get_route_id()
        .await
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    let delta_state = DeltaState::with_route(project_id, route_id);
    let node = delta_state.claim_node(&node_id, &body.worker_name).await?;

    Ok(Json(node))
}

#[derive(Debug, Deserialize)]
pub struct CompleteNodeRequest {
    pub worker_name: String,
}

/// Complete a node
///
/// POST /api/runtimes/{name}/nodes/{id}/complete
pub async fn complete_node(
    Path((name, node_id)): Path<(String, String)>,
    Json(body): Json<CompleteNodeRequest>,
) -> Result<Json<crate::core::delta::BoardNode>> {
    use crate::core::delta::DeltaState;
    use crate::core::{config, state::SQLiteState};

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let state = SQLiteState::new(&name).await?;

    let project_id = state.get_project_id().await?.ok_or_else(|| {
        OrchestratorError::InvalidOperation("Run is not linked to a project".into())
    })?;
    let route_id = state
        .get_route_id()
        .await
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    let delta_state = DeltaState::with_route(project_id, route_id);
    let node = delta_state
        .complete_node(&node_id, &body.worker_name)
        .await?;

    Ok(Json(node))
}

/// Unclaim a node
///
/// POST /api/runtimes/{name}/nodes/{id}/unclaim
pub async fn unclaim_node(Path((name, node_id)): Path<(String, String)>) -> Result<StatusCode> {
    use crate::core::delta::DeltaState;
    use crate::core::{config, state::SQLiteState};

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let state = SQLiteState::new(&name).await?;

    let project_id = state.get_project_id().await?.ok_or_else(|| {
        OrchestratorError::InvalidOperation("Run is not linked to a project".into())
    })?;
    let route_id = state
        .get_route_id()
        .await
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    let delta_state = DeltaState::with_route(project_id, route_id);
    delta_state.unclaim_node(&node_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
pub struct NodeBlockedResponse {
    pub blocked: bool,
}

/// Check if a node is blocked
///
/// GET /api/runtimes/{name}/nodes/{id}/blocked
pub async fn is_node_blocked(
    Path((name, node_id)): Path<(String, String)>,
) -> Result<Json<NodeBlockedResponse>> {
    use crate::core::delta::DeltaState;
    use crate::core::{config, state::SQLiteState};

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let state = SQLiteState::new(&name).await?;

    let project_id = state.get_project_id().await?.ok_or_else(|| {
        OrchestratorError::InvalidOperation("Run is not linked to a project".into())
    })?;
    let route_id = state
        .get_route_id()
        .await
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    let delta_state = DeltaState::with_route(project_id, route_id);
    let blocked = delta_state.is_node_blocked(&node_id).await?;

    Ok(Json(NodeBlockedResponse { blocked }))
}

#[derive(Debug, Deserialize)]
pub struct CheckPassRequest {
    pub worker_name: String,
}

/// Mark a check node as passed
///
/// POST /api/runtimes/{name}/nodes/{id}/check-pass
pub async fn node_check_pass(
    Path((name, check_id)): Path<(String, String)>,
    Json(body): Json<CheckPassRequest>,
) -> Result<StatusCode> {
    use crate::core::delta::DeltaState;
    use crate::core::{config, state::SQLiteState};

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let state = SQLiteState::new(&name).await?;

    let project_id = state.get_project_id().await?.ok_or_else(|| {
        OrchestratorError::InvalidOperation("Run is not linked to a project".into())
    })?;
    let route_id = state
        .get_route_id()
        .await
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    let delta_state = DeltaState::with_route(project_id, route_id);
    delta_state.check_pass(&check_id, &body.worker_name).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct CheckFailRequest {
    pub worker_name: String,
    pub feedback: String,
}

#[derive(Debug, Serialize)]
pub struct CheckFailResponse {
    pub repair_node_id: String,
}

/// Mark a check node as failed, creating a repair node
///
/// POST /api/runtimes/{name}/nodes/{id}/check-fail
pub async fn node_check_fail(
    Path((name, check_id)): Path<(String, String)>,
    Json(body): Json<CheckFailRequest>,
) -> Result<Json<CheckFailResponse>> {
    use crate::core::delta::DeltaState;
    use crate::core::{config, state::SQLiteState};

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let state = SQLiteState::new(&name).await?;

    let project_id = state.get_project_id().await?.ok_or_else(|| {
        OrchestratorError::InvalidOperation("Run is not linked to a project".into())
    })?;
    let route_id = state
        .get_route_id()
        .await
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    let delta_state = DeltaState::with_route(project_id, route_id);
    let repair_node_id = delta_state
        .check_fail(&check_id, &body.worker_name, &body.feedback)
        .await?;

    Ok(Json(CheckFailResponse { repair_node_id }))
}

#[derive(Debug, Deserialize)]
pub struct SetTokensRequest {
    pub tokens: i64,
}

/// Set tokens used on a node
///
/// POST /api/runtimes/{name}/nodes/{id}/tokens
pub async fn set_node_tokens(
    Path((name, node_id)): Path<(String, String)>,
    Json(body): Json<SetTokensRequest>,
) -> Result<StatusCode> {
    use crate::core::delta::DeltaState;
    use crate::core::{config, state::SQLiteState};

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let state = SQLiteState::new(&name).await?;

    let project_id = state.get_project_id().await?.ok_or_else(|| {
        OrchestratorError::InvalidOperation("Run is not linked to a project".into())
    })?;
    let route_id = state
        .get_route_id()
        .await
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    let delta_state = DeltaState::with_route(project_id, route_id);
    delta_state.set_node_tokens(&node_id, body.tokens).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
pub struct ValidatedNodesResponse {
    pub node_ids: Vec<String>,
}

/// Get all node IDs validated by an eval node
///
/// GET /api/runtimes/{name}/nodes/{id}/validated
pub async fn get_validated_nodes(
    Path((name, eval_id)): Path<(String, String)>,
) -> Result<Json<ValidatedNodesResponse>> {
    use crate::core::delta::DeltaState;
    use crate::core::{config, state::SQLiteState};

    let runtime_dir = config::runtime_dir(&name);
    if !runtime_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let state = SQLiteState::new(&name).await?;

    let project_id = state.get_project_id().await?.ok_or_else(|| {
        OrchestratorError::InvalidOperation("Run is not linked to a project".into())
    })?;
    let route_id = state
        .get_route_id()
        .await
        .map_err(|e| OrchestratorError::State(e.to_string()))?;

    let delta_state = DeltaState::with_route(project_id, route_id);
    let node_ids = delta_state.get_checked_nodes(&eval_id).await?;

    Ok(Json(ValidatedNodesResponse { node_ids }))
}
