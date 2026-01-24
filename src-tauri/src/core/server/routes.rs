//! API route handlers

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::AppState;
use crate::core::api_types::{
    ConfigResponse, Eval, HistoryEntry, Message, RunDetail, RunSummary, Task, ThreadSummary,
    Worker, WorkerEventsResponse,
};
use crate::core::orchestrator::{
    AddTaskRequest, CreateRunRequest, CreateRunResponse, DeliverRunRequest, HealthResponse,
    Orchestrator, OrchestratorError, ResumeRunRequest, ResumeWorkerRequest, SendMessageRequest,
    SpawnSingleWorkerRequest, SpawnWorkersRequest, SpawnWorkersResponse,
};

/// Convert OrchestratorError to HTTP response
impl IntoResponse for OrchestratorError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            OrchestratorError::RunNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            OrchestratorError::WorkerNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            OrchestratorError::TaskNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            OrchestratorError::InvalidOperation(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            OrchestratorError::UnknownProfile(_) => (StatusCode::BAD_REQUEST, self.to_string()),
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

pub async fn create_run(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRunRequest>,
) -> Result<Json<CreateRunResponse>> {
    let response = state.orchestrator.create_run(body).await?;
    Ok(Json(response))
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

/// Upload working directory as tarball
///
/// Accepts a gzipped tar archive of the project files.
/// Extracts to the run's work directory for workers to use.
pub async fn upload_files(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    body: Bytes,
) -> Result<StatusCode> {
    state
        .orchestrator
        .upload_files(&name, body.to_vec())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

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

    let run_dir = config::run_dir(&name);
    if !run_dir.exists() {
        return Err(OrchestratorError::RunNotFound(name));
    }

    let work_dir = run_dir.join("work");
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

/// Spawn workers for a run
///
/// Creates and starts the specified number of workers.
/// The run must have files uploaded first.
pub async fn spawn_workers(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<SpawnWorkersRequest>,
) -> Result<Json<SpawnWorkersResponse>> {
    let response = state.orchestrator.spawn_workers(&name, body.count).await?;
    Ok(Json(response))
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

// =============================================================================
// Tasks
// =============================================================================

pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<Vec<Task>>> {
    let tasks = state.orchestrator.list_tasks(&name).await?;
    Ok(Json(tasks))
}

pub async fn add_task(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<AddTaskRequest>,
) -> Result<Json<Task>> {
    let task = state.orchestrator.add_task(&name, &body.content).await?;
    Ok(Json(task))
}

pub async fn delete_task(
    State(state): State<Arc<AppState>>,
    Path((name, task_id)): Path<(String, String)>,
) -> Result<StatusCode> {
    state.orchestrator.delete_task(&name, &task_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn complete_task(
    State(state): State<Arc<AppState>>,
    Path((name, task_id)): Path<(String, String)>,
) -> Result<StatusCode> {
    state.orchestrator.complete_task(&name, &task_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn reopen_task(
    State(state): State<Arc<AppState>>,
    Path((name, task_id)): Path<(String, String)>,
) -> Result<StatusCode> {
    state.orchestrator.reopen_task(&name, &task_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Messages
// =============================================================================

pub async fn list_threads(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<Vec<ThreadSummary>>> {
    let threads = state.orchestrator.list_threads(&name).await?;
    Ok(Json(threads))
}

pub async fn get_messages(
    State(state): State<Arc<AppState>>,
    Path((name, thread)): Path<(String, String)>,
) -> Result<Json<Vec<Message>>> {
    let messages = state.orchestrator.get_messages(&name, &thread).await?;
    Ok(Json(messages))
}

pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Path((name, thread)): Path<(String, String)>,
    Json(body): Json<SendMessageRequest>,
) -> Result<Json<Message>> {
    let message = state
        .orchestrator
        .send_message(&name, &thread, &body.content)
        .await?;
    Ok(Json(message))
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
    mask_credential, AgentAuthConfigRequest, AgentConfigRequest, CompactionConfigRequest,
    CredentialStatusResponse, GeneralConfigRequest, GitConfigRequest, RunnerConfigResponse,
    StoreCredentialRequest,
};
use crate::core::config::OrchestratorProfile;
use crate::core::credentials::CredentialStore;
use crate::core::runner::RunnerConfig;

/// Patch general configuration settings
pub async fn patch_general_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<GeneralConfigRequest>,
) -> Result<StatusCode> {
    let mut config = state.config.write().await;
    config.update_general(
        body.eval_timeout,
        body.auto_learn,
        body.max_iterations,
        body.human_in_the_loop,
        body.default_runner,
        body.coordinator_port,
    );
    config
        .save()
        .map_err(|e| OrchestratorError::Config(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Patch agent configuration settings
pub async fn patch_agent_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AgentConfigRequest>,
) -> Result<StatusCode> {
    let mut config = state.config.write().await;
    config.update_agent(body.command);
    config
        .save()
        .map_err(|e| OrchestratorError::Config(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Patch compaction configuration settings
pub async fn patch_compaction_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CompactionConfigRequest>,
) -> Result<StatusCode> {
    let mut config = state.config.write().await;
    config.update_compaction(body.enabled, body.threshold, body.keep_messages);
    config
        .save()
        .map_err(|e| OrchestratorError::Config(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Get full auth configuration
pub async fn get_auth_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<crate::core::api_types::AuthConfigResponse>> {
    let config = state.config.read().await;
    Ok(Json(config.auth.clone().into()))
}

/// Patch auth configuration for a specific agent
pub async fn patch_agent_auth(
    State(state): State<Arc<AppState>>,
    Path(agent): Path<String>,
    Json(body): Json<AgentAuthConfigRequest>,
) -> Result<StatusCode> {
    let mut config = state.config.write().await;
    config.update_agent_auth(&agent, body.into());
    config
        .save()
        .map_err(|e| OrchestratorError::Config(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete auth configuration for a specific agent
pub async fn delete_agent_auth(
    State(state): State<Arc<AppState>>,
    Path(agent): Path<String>,
) -> Result<StatusCode> {
    let mut config = state.config.write().await;
    config.delete_agent_auth(&agent);
    config
        .save()
        .map_err(|e| OrchestratorError::Config(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Runners CRUD
// =============================================================================

/// List all configured runners
pub async fn list_runners(
    State(state): State<Arc<AppState>>,
) -> Result<Json<std::collections::HashMap<String, RunnerConfigResponse>>> {
    let config = state.config.read().await;
    let mut runners = std::collections::HashMap::new();

    // Always include "local" as a built-in runner
    runners.insert(
        "local".to_string(),
        RunnerConfigResponse {
            host: crate::core::api_types::HostConfigResponse::Local,
            container: None,
        },
    );

    // Add configured runners
    for (name, runner_config) in &config.runners {
        runners.insert(name.clone(), runner_config.clone().into());
    }

    Ok(Json(runners))
}

/// Get a specific runner by name
pub async fn get_runner(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<RunnerConfigResponse>> {
    let config = state.config.read().await;

    if name == "local" {
        return Ok(Json(RunnerConfigResponse {
            host: crate::core::api_types::HostConfigResponse::Local,
            container: None,
        }));
    }

    let runner = config
        .runners
        .get(&name)
        .ok_or_else(|| OrchestratorError::Other(format!("Runner '{}' not found", name)))?;

    Ok(Json(runner.clone().into()))
}

/// Create or update a runner
pub async fn put_runner(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<RunnerConfigResponse>,
) -> Result<StatusCode> {
    if name == "local" {
        return Err(OrchestratorError::InvalidOperation(
            "Cannot modify built-in 'local' runner".into(),
        ));
    }

    let mut config = state.config.write().await;
    let runner_config: RunnerConfig = body.into();
    config.runners.insert(name, runner_config);
    config
        .save()
        .map_err(|e| OrchestratorError::Config(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete a runner
pub async fn delete_runner(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<StatusCode> {
    if name == "local" {
        return Err(OrchestratorError::InvalidOperation(
            "Cannot delete built-in 'local' runner".into(),
        ));
    }

    let mut config = state.config.write().await;
    config.runners.remove(&name);
    config
        .save()
        .map_err(|e| OrchestratorError::Config(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Profiles CRUD
// =============================================================================

/// List all configured profiles
pub async fn list_profiles(
    State(state): State<Arc<AppState>>,
) -> Result<
    Json<std::collections::HashMap<String, crate::core::api_types::OrchestratorProfileResponse>>,
> {
    let config = state.config.read().await;
    let profiles = config
        .profiles
        .iter()
        .map(|(name, profile)| (name.clone(), profile.clone().into()))
        .collect();
    Ok(Json(profiles))
}

/// Get a specific profile by name
pub async fn get_profile(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<crate::core::api_types::OrchestratorProfileResponse>> {
    let config = state.config.read().await;
    let profile = config
        .profiles
        .get(&name)
        .ok_or_else(|| OrchestratorError::Other(format!("Profile '{}' not found", name)))?;
    Ok(Json(profile.clone().into()))
}

/// Profile update request (with unmasked secrets)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileUpdateRequest {
    pub mode: crate::core::api_types::OrchestratorModeResponse,
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub access: Option<crate::core::config::OrchestratorAccess>,
}

/// Create or update a profile
pub async fn put_profile(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<ProfileUpdateRequest>,
) -> Result<StatusCode> {
    let mut config = state.config.write().await;

    let profile = OrchestratorProfile {
        mode: body.mode.into(),
        url: body.url,
        api_key: body.api_key,
        access: body.access.unwrap_or_default(),
    };

    config.profiles.insert(name, profile);
    config
        .save()
        .map_err(|e| OrchestratorError::Config(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete a profile
pub async fn delete_profile(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<StatusCode> {
    if name == "local" {
        return Err(OrchestratorError::InvalidOperation(
            "Cannot delete built-in 'local' profile".into(),
        ));
    }

    let mut config = state.config.write().await;
    config.profiles.remove(&name);
    config
        .save()
        .map_err(|e| OrchestratorError::Config(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Git Config
// =============================================================================

/// Patch git configuration
pub async fn patch_git_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<GitConfigRequest>,
) -> Result<StatusCode> {
    let mut config = state.config.write().await;
    if let Some(provider) = body.default_provider {
        config.git.default_provider = Some(provider.into());
    }
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
        .map_err(|e| OrchestratorError::Other(format!("Failed to open credential store: {}", e)))?;

    store
        .store(&key, &body.value)
        .map_err(|e| OrchestratorError::Other(format!("Failed to store credential: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Get credential status (masked value)
pub async fn get_credential(Path(key): Path<String>) -> Result<Json<CredentialStatusResponse>> {
    let store = CredentialStore::open()
        .map_err(|e| OrchestratorError::Other(format!("Failed to open credential store: {}", e)))?;

    match store.load(&key) {
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
        .map_err(|e| OrchestratorError::Other(format!("Failed to open credential store: {}", e)))?;

    store
        .delete(&key)
        .map_err(|e| OrchestratorError::Other(format!("Failed to delete credential: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}
