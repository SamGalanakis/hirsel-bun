//! Shared API types
//!
//! Types used by the orchestrator, server, and GUI commands.
//! These are shared to allow CLI-only builds without the gui feature.

use serde::{Deserialize, Serialize};

use crate::core::config;

// =============================================================================
// Status Enums
// =============================================================================

/// Run status values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Draft,
    Working,
    Paused,
    Failed,
    Eval,
    Done,
    Delivered,
}

/// Worker status values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Working,
    Awaiting,
    Paused,
    Error,
}

/// Worker location
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerLocation {
    Local,
    Remote,
}

/// Eval status values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalStatus {
    Running,
    Passed,
    Failed,
}

// =============================================================================
// Response Types
// =============================================================================

/// Summary of a run for the run list panel
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub name: String,
    pub status: RunStatus,
    pub tasks_done: u32,
    pub tasks_total: u32,
    pub workers_active: u32,
    pub workers_total: u32,
    /// Desired worker count derived from worker_scale (for UI display).
    pub workers_desired: u32,
    pub elapsed_minutes: f64,
    pub time_limit_minutes: Option<u32>,
    pub has_unread_messages: bool,
    pub created_at: String,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
}

/// Full run details for the detail view
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDetail {
    pub name: String,
    pub status: RunStatus,
    pub request: Option<String>,
    pub project_path: Option<String>,
    pub remote_url: Option<String>,
    pub branch: Option<String>,
    pub worker_scale: Option<String>,
    pub time_limit_minutes: Option<u32>,
    pub started_at: Option<String>,
    pub summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub iteration_count: u32,
    pub human_in_the_loop: bool,
    pub waiting_reason: Option<String>,
    pub unread_count: u32,
    pub tasks_done: u32,
    pub tasks_total: u32,
    pub workers_active: u32,
    pub workers_total: u32,
    /// Desired worker count derived from worker_scale (for UI display).
    pub workers_desired: u32,
    pub elapsed_minutes: f64,
    pub agent_type: String,
    pub metrics_available: bool,
    pub runner: Option<String>,
    pub worker_runners: Option<std::collections::HashMap<String, String>>,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
}

/// Sheep avatar configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheepConfig {
    pub hat: u8,
    pub fluffiness: u8,
    pub body_width: i8,
    pub body_height: i8,
    pub ear_position: i8,
    pub leg_length: i8,
    pub hue_shift: u16,
    pub glasses: u8,
    pub bowtie: u8,
}

impl SheepConfig {
    /// Generate deterministic config from worker name
    pub fn from_name(name: &str, is_leader: bool) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        let hash = hasher.finish();

        let bytes = hash.to_le_bytes();

        SheepConfig {
            hat: if is_leader { 1 } else { bytes[0] % 8 },
            fluffiness: bytes[1] % 4,
            body_width: ((bytes[2] % 5) as i8) - 2,
            body_height: ((bytes[3] % 5) as i8) - 2,
            ear_position: ((bytes[4] % 3) as i8) - 1,
            leg_length: ((bytes[5] % 3) as i8) - 1,
            hue_shift: 0,
            glasses: bytes[6] % 5,
            bowtie: bytes[7] % 5,
        }
    }

    /// Generate deterministic config for a check agent
    pub fn for_check(check_id: u32) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        check_id.hash(&mut hasher);
        let hash = hasher.finish();

        let bytes = hash.to_le_bytes();

        SheepConfig {
            hat: 8, // Always detective hat
            fluffiness: bytes[1] % 4,
            body_width: ((bytes[2] % 5) as i8) - 2,
            body_height: ((bytes[3] % 5) as i8) - 2,
            ear_position: ((bytes[4] % 3) as i8) - 1,
            leg_length: ((bytes[5] % 3) as i8) - 1,
            hue_shift: 0,
            glasses: bytes[6] % 5,
            bowtie: bytes[7] % 5,
        }
    }
}

/// Worker from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worker {
    pub id: u32,
    pub name: String,
    pub pid: Option<u32>,
    pub session_id: Option<String>,
    pub status: WorkerStatus,
    pub work_dir: Option<String>,
    pub waiting_thread: Option<String>,
    pub location: WorkerLocation,
    pub last_heartbeat: Option<String>,
    pub created_at: String,
    pub needs_restart: bool,
    pub session_started_at: Option<String>,
    pub hitl_waiting: bool,
    pub is_leader: bool,
    pub context_utilization: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub turns: Option<u32>,
    pub current_task: Option<String>,
    pub sheep_config: SheepConfig,
}

/// Message from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: u32,
    pub thread: String,
    pub sender: String,
    pub content: String,
    pub waiting: bool,
    pub read_by: Option<Vec<String>>,
    pub timestamp: String,
}

/// Thread summary for chat panel
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub name: String,
    pub message_count: u32,
    pub unread_count: u32,
    pub last_message: Option<String>,
    pub last_timestamp: Option<String>,
}

/// History entry for activity log
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: u32,
    pub timestamp: String,
    pub action: String,
    pub detail: Option<String>,
}

/// Eval from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Eval {
    pub id: u32,
    pub branch: String,
    pub eval_name: Option<String>,
    pub status: EvalStatus,
    pub feedback: Option<String>,
    pub log_file: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub sheep_config: SheepConfig,
}

// =============================================================================
// LLM Config Types
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProviderResponse {
    Codex,
    Openrouter,
}

impl From<config::LlmProvider> for LlmProviderResponse {
    fn from(value: config::LlmProvider) -> Self {
        match value {
            config::LlmProvider::Codex => Self::Codex,
            config::LlmProvider::Openrouter => Self::Openrouter,
        }
    }
}

impl From<LlmProviderResponse> for config::LlmProvider {
    fn from(value: LlmProviderResponse) -> Self {
        match value {
            LlmProviderResponse::Codex => Self::Codex,
            LlmProviderResponse::Openrouter => Self::Openrouter,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfigResponse {
    pub provider: LlmProviderResponse,
    pub openrouter_base_url: Option<String>,
}

impl From<config::LlmConfig> for LlmConfigResponse {
    fn from(value: config::LlmConfig) -> Self {
        Self {
            provider: value.provider.into(),
            openrouter_base_url: value.openrouter_base_url,
        }
    }
}

impl From<LlmConfigResponse> for config::LlmConfig {
    fn from(value: LlmConfigResponse) -> Self {
        Self {
            provider: value.provider.into(),
            openrouter_base_url: value.openrouter_base_url,
        }
    }
}

// =============================================================================
// Runner Config Types (Single-Host Model)
// =============================================================================

/// Container configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerConfigResponse {
    pub image: String,
}

/// Runner configuration for frontend (single-host model)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerConfigResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerConfigResponse>,
}

impl From<crate::core::runner::RunnerConfig> for RunnerConfigResponse {
    fn from(cfg: crate::core::runner::RunnerConfig) -> Self {
        let container = cfg
            .container
            .map(|c| ContainerConfigResponse { image: c.image });

        RunnerConfigResponse { container }
    }
}

impl From<RunnerConfigResponse> for crate::core::runner::RunnerConfig {
    fn from(cfg: RunnerConfigResponse) -> Self {
        use crate::core::runner::{ContainerConfig, RunnerConfig};

        let container = cfg.container.map(|c| ContainerConfig { image: c.image });

        RunnerConfig { container }
    }
}

// =============================================================================
// Backend Connection Types
// =============================================================================

/// Backend connection settings for the frontend/client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendConfigResponse {
    pub url: Option<String>,
    pub api_key: Option<String>,
}

impl From<config::BackendConfig> for BackendConfigResponse {
    fn from(backend: config::BackendConfig) -> Self {
        Self {
            url: backend.url,
            api_key: backend.api_key,
        }
    }
}

// =============================================================================
// Storage Types
// =============================================================================

/// Storage provider type for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageProviderResponse {
    S3,
    Minio,
}

impl From<config::StorageProvider> for StorageProviderResponse {
    fn from(provider: config::StorageProvider) -> Self {
        match provider {
            config::StorageProvider::S3 => Self::S3,
            config::StorageProvider::Minio => Self::Minio,
        }
    }
}

impl From<StorageProviderResponse> for config::StorageProvider {
    fn from(provider: StorageProviderResponse) -> Self {
        match provider {
            StorageProviderResponse::S3 => Self::S3,
            StorageProviderResponse::Minio => Self::Minio,
        }
    }
}

/// S3 configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3ConfigResponse {
    pub provider: StorageProviderResponse,
    pub endpoint: Option<String>,
    pub bucket: String,
    pub region: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
}

impl From<&config::S3Config> for S3ConfigResponse {
    fn from(cfg: &config::S3Config) -> Self {
        Self {
            provider: cfg.provider.into(),
            endpoint: cfg.endpoint.clone(),
            bucket: cfg.bucket.clone(),
            region: cfg.region.clone(),
            access_key_id: cfg.access_key_id.clone(),
            secret_access_key: cfg.secret_access_key.clone(),
        }
    }
}

impl From<S3ConfigResponse> for config::S3Config {
    fn from(cfg: S3ConfigResponse) -> Self {
        Self {
            provider: cfg.provider.into(),
            endpoint: cfg.endpoint,
            bucket: cfg.bucket,
            region: cfg.region,
            access_key_id: cfg.access_key_id,
            secret_access_key: cfg.secret_access_key,
        }
    }
}

/// Storage backend type for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackendResponse {
    Local,
    S3,
}

impl From<config::StorageBackend> for StorageBackendResponse {
    fn from(backend: config::StorageBackend) -> Self {
        match backend {
            config::StorageBackend::Local => Self::Local,
            config::StorageBackend::S3 => Self::S3,
        }
    }
}

impl From<StorageBackendResponse> for config::StorageBackend {
    fn from(backend: StorageBackendResponse) -> Self {
        match backend {
            StorageBackendResponse::Local => Self::Local,
            StorageBackendResponse::S3 => Self::S3,
        }
    }
}

/// Storage configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageConfigResponse {
    pub files: StorageBackendResponse,
    pub storages: std::collections::HashMap<String, S3ConfigResponse>,
    pub default_storage: Option<String>,
}

impl From<&config::StorageConfig> for StorageConfigResponse {
    fn from(cfg: &config::StorageConfig) -> Self {
        Self {
            files: cfg.files.into(),
            storages: cfg
                .storages
                .iter()
                .map(|(k, v)| (k.clone(), v.into()))
                .collect(),
            default_storage: cfg.default_storage.clone(),
        }
    }
}

impl From<StorageConfigResponse> for config::StorageConfig {
    fn from(cfg: StorageConfigResponse) -> Self {
        Self {
            files: cfg.files.into(),
            storages: cfg
                .storages
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
            default_storage: cfg.default_storage,
        }
    }
}

// =============================================================================
// Git Provider Types
// =============================================================================

/// Git provider type for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitProviderResponse {
    Github,
    // Future: Gitlab, Bitbucket, etc.
}

impl From<config::GitProvider> for GitProviderResponse {
    fn from(provider: config::GitProvider) -> Self {
        match provider {
            config::GitProvider::Github => Self::Github,
        }
    }
}

impl From<GitProviderResponse> for config::GitProvider {
    fn from(provider: GitProviderResponse) -> Self {
        match provider {
            GitProviderResponse::Github => Self::Github,
        }
    }
}

/// Git configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitConfigResponse {
    pub default_provider: Option<GitProviderResponse>,
    /// Map of provider -> whether a token is configured (from CredentialStore)
    pub configured_providers: Vec<GitProviderResponse>,
}

/// Application configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigResponse {
    pub runs_dir: String,
    pub agent_command: Vec<String>,
    pub eval_timeout: u32,
    pub auto_learn: bool,
    pub user_message_pause: String,
    pub human_in_the_loop: bool,
    pub context_warning_threshold: f64,
    pub coordinator_port: u16,
    pub llm: LlmConfigResponse,
    pub runners: std::collections::HashMap<String, RunnerConfigResponse>,
    pub default_runner: Option<String>,
    pub worker_runners: std::collections::HashMap<String, String>,
    pub backend: BackendConfigResponse,
    pub git: GitConfigResponse,
    pub storage: StorageConfigResponse,
}

// =============================================================================
// Worker Event Types
// =============================================================================

/// Worker event for real-time streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEventResponse {
    pub id: i64,
    pub worker_name: String,
    pub event_type: String,
    pub timestamp: String,
    pub content: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_title: Option<String>,
    pub tool_kind: Option<String>,
    pub tool_status: Option<String>,
    pub tool_input: Option<String>,
    pub tool_output: Option<String>,
}

/// Response for worker events query
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEventsResponse {
    pub events: Vec<WorkerEventResponse>,
    pub last_id: Option<i64>,
    /// Worker status for determining if still streaming
    pub worker_status: Option<String>,
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Convert core Status to API RunStatus
impl From<crate::core::state::Status> for RunStatus {
    fn from(status: crate::core::state::Status) -> Self {
        match status {
            crate::core::state::Status::Draft => RunStatus::Draft,
            crate::core::state::Status::Working => RunStatus::Working,
            crate::core::state::Status::Paused => RunStatus::Paused,
            crate::core::state::Status::Failed => RunStatus::Failed,
            crate::core::state::Status::Eval => RunStatus::Eval,
            crate::core::state::Status::Done => RunStatus::Done,
            crate::core::state::Status::Delivered => RunStatus::Delivered,
        }
    }
}

/// Helper function for converting Status (delegates to From impl)
pub fn convert_status(status: crate::core::state::Status) -> RunStatus {
    status.into()
}

/// Convert core WorkerStatus to API WorkerStatus
impl From<crate::core::state::WorkerStatus> for WorkerStatus {
    fn from(s: crate::core::state::WorkerStatus) -> Self {
        match s {
            crate::core::state::WorkerStatus::Working => Self::Working,
            crate::core::state::WorkerStatus::Awaiting => Self::Awaiting,
            crate::core::state::WorkerStatus::Paused => Self::Paused,
            crate::core::state::WorkerStatus::Error => Self::Error,
        }
    }
}

/// Parse an RFC3339 timestamp and return elapsed minutes since then
pub fn parse_elapsed_minutes(timestamp_str: &str) -> f64 {
    if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(timestamp_str) {
        let now = chrono::Utc::now();
        let duration = now.signed_duration_since(ts);
        duration.num_seconds() as f64 / 60.0
    } else {
        0.0
    }
}

/// Parse a timestamp string to chrono DateTime
pub fn parse_timestamp(timestamp: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

// =============================================================================
// Config Update Request Types
// =============================================================================

/// Request to update general configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralConfigRequest {
    pub eval_timeout: Option<u32>,
    pub auto_learn: Option<bool>,
    pub human_in_the_loop: Option<bool>,
    pub default_runner: Option<Option<String>>,
    pub coordinator_port: Option<u16>,
}

/// Request to update agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigRequest {
    pub command: Option<Vec<String>>,
}

/// Request to update LLM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfigRequest {
    pub provider: Option<LlmProviderResponse>,
    pub openrouter_base_url: Option<Option<String>>,
}

impl LlmConfigRequest {
    pub fn apply(self, config: &mut config::LlmConfig) {
        if let Some(provider) = self.provider {
            config.provider = provider.into();
        }
        if let Some(base_url) = self.openrouter_base_url {
            config.openrouter_base_url = base_url.and_then(|v| {
                let trimmed = v.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            });
        }
    }
}

/// Request to update git configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitConfigRequest {
    pub default_provider: Option<GitProviderResponse>,
}

/// Request to store a credential
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreCredentialRequest {
    pub value: String,
}

/// Response for credential status check
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStatusResponse {
    pub key: String,
    pub exists: bool,
    pub masked_value: Option<String>,
}

/// Helper to mask a credential value for display
pub fn mask_credential(value: &str) -> String {
    if value.len() > 8 {
        format!("{}...{}", &value[..4], &value[value.len() - 4..])
    } else {
        "****".to_string()
    }
}
