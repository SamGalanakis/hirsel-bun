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

/// Task status values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    Doing,
    Done,
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
    pub elapsed_minutes: f64,
    pub time_limit_minutes: Option<u32>,
    pub has_unread_messages: bool,
    pub created_at: String,
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
    pub max_iterations: Option<u32>,
    pub human_in_the_loop: bool,
    pub waiting_reason: Option<String>,
    pub unread_count: u32,
    pub tasks_done: u32,
    pub tasks_total: u32,
    pub workers_active: u32,
    pub workers_total: u32,
    pub elapsed_minutes: f64,
    pub learnings_count: u32,
    pub learnings_processed_at: Option<String>,
    pub agent_type: String,
    pub metrics_available: bool,
    pub runner: Option<String>,
    pub worker_runners: Option<std::collections::HashMap<String, String>>,
}

/// Task from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub completed_at: Option<String>,
    pub parent_id: Option<String>,
    pub blocked_by: Option<Vec<String>>,
    pub tokens_used: Option<u64>,
    pub created_at: String,
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

    /// Generate deterministic config for an eval agent
    pub fn for_eval(eval_id: u32) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        eval_id.hash(&mut hasher);
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
// Auth Types
// =============================================================================

/// Authentication method for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethodResponse {
    Env,
    ApiKey,
    OAuth,
}

impl From<config::AuthMethod> for AuthMethodResponse {
    fn from(method: config::AuthMethod) -> Self {
        match method {
            config::AuthMethod::Env => Self::Env,
            config::AuthMethod::ApiKey => Self::ApiKey,
            config::AuthMethod::OAuth => Self::OAuth,
        }
    }
}

impl From<AuthMethodResponse> for config::AuthMethod {
    fn from(method: AuthMethodResponse) -> Self {
        match method {
            AuthMethodResponse::Env => Self::Env,
            AuthMethodResponse::ApiKey => Self::ApiKey,
            AuthMethodResponse::OAuth => Self::OAuth,
        }
    }
}

/// Agent auth configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthResponse {
    pub method: AuthMethodResponse,
    pub api_key: Option<String>,
    pub env_var: Option<String>,
}

impl From<config::AgentAuth> for AgentAuthResponse {
    fn from(auth: config::AgentAuth) -> Self {
        Self {
            method: auth.method.into(),
            api_key: auth.api_key.map(|k| {
                if k.len() > 8 {
                    format!("{}...{}", &k[..4], &k[k.len() - 4..])
                } else {
                    "****".to_string()
                }
            }),
            env_var: auth.env_var,
        }
    }
}

/// Auth configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigResponse {
    pub default_method: AuthMethodResponse,
    pub claude: Option<AgentAuthResponse>,
    pub gemini: Option<AgentAuthResponse>,
    pub codex: Option<AgentAuthResponse>,
    pub goose: Option<AgentAuthResponse>,
}

impl From<config::AuthConfig> for AuthConfigResponse {
    fn from(auth: config::AuthConfig) -> Self {
        Self {
            default_method: auth.default_method.into(),
            claude: auth.claude.map(|a| a.into()),
            gemini: auth.gemini.map(|a| a.into()),
            codex: auth.codex.map(|a| a.into()),
            goose: auth.goose.map(|a| a.into()),
        }
    }
}

// =============================================================================
// Runner Config Types (Host + Container Model)
// =============================================================================

/// Container configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerConfigResponse {
    pub image: String,
}

/// Host configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum HostConfigResponse {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "client")]
    Client,
    #[serde(rename = "ssh")]
    Ssh {
        address: String,
        port: u16,
        ssh_key: Option<String>,
        work_base: String,
        location: Option<String>,
    },
    #[serde(rename = "fly")]
    Fly {
        api_token: Option<String>,
        app: String,
        region: Option<String>,
        #[serde(default = "default_fly_cpu_kind")]
        cpu_kind: String,
        #[serde(default = "default_fly_cpus")]
        cpus: u32,
        #[serde(default = "default_fly_memory_mb")]
        memory_mb: u32,
        #[serde(default = "default_auto_destroy")]
        auto_destroy: bool,
    },
}

/// Runner configuration for frontend (Host + Container model)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerConfigResponse {
    pub host: HostConfigResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerConfigResponse>,
}

impl From<crate::core::runner::RunnerConfig> for RunnerConfigResponse {
    fn from(cfg: crate::core::runner::RunnerConfig) -> Self {
        use crate::core::runner::{HostConfig, HostConfigOrShortcut};

        let host = match &cfg.host {
            HostConfigOrShortcut::Shortcut(s) => match s.to_lowercase().as_str() {
                "client" => HostConfigResponse::Client,
                _ => HostConfigResponse::Local,
            },
            HostConfigOrShortcut::Full(h) => match h {
                HostConfig::Local => HostConfigResponse::Local,
                HostConfig::Client => HostConfigResponse::Client,
                HostConfig::Ssh(ssh) => HostConfigResponse::Ssh {
                    address: ssh.address.clone(),
                    port: ssh.port,
                    ssh_key: ssh.ssh_key.clone(),
                    work_base: ssh.work_base.clone(),
                    location: ssh.location.clone(),
                },
                HostConfig::Fly(fly) => HostConfigResponse::Fly {
                    api_token: fly.api_token.clone(),
                    app: fly.app.clone(),
                    region: fly.region.clone(),
                    cpu_kind: fly.cpu_kind.clone(),
                    cpus: fly.cpus,
                    memory_mb: fly.memory_mb,
                    auto_destroy: fly.auto_destroy,
                },
            },
        };

        let container = cfg
            .container
            .map(|c| ContainerConfigResponse { image: c.image });

        RunnerConfigResponse { host, container }
    }
}

impl From<RunnerConfigResponse> for crate::core::runner::RunnerConfig {
    fn from(cfg: RunnerConfigResponse) -> Self {
        use crate::core::runner::{
            ContainerConfig, FlyHostConfig, HostConfig, HostConfigOrShortcut, RunnerConfig,
            SshHostConfig,
        };

        let host = match cfg.host {
            HostConfigResponse::Local => HostConfigOrShortcut::Shortcut("local".to_string()),
            HostConfigResponse::Client => HostConfigOrShortcut::Shortcut("client".to_string()),
            HostConfigResponse::Ssh {
                address,
                port,
                ssh_key,
                work_base,
                location,
            } => HostConfigOrShortcut::Full(HostConfig::Ssh(SshHostConfig {
                address,
                port,
                ssh_key,
                work_base,
                location,
            })),
            HostConfigResponse::Fly {
                api_token,
                app,
                region,
                cpu_kind,
                cpus,
                memory_mb,
                auto_destroy,
            } => HostConfigOrShortcut::Full(HostConfig::Fly(FlyHostConfig {
                api_token,
                app,
                region,
                cpu_kind,
                cpus,
                memory_mb,
                auto_destroy,
            })),
        };

        let container = cfg.container.map(|c| ContainerConfig { image: c.image });

        RunnerConfig { host, container }
    }
}

fn default_auto_destroy() -> bool {
    true
}
fn default_fly_cpu_kind() -> String {
    "shared".to_string()
}
fn default_fly_cpus() -> u32 {
    1
}
fn default_fly_memory_mb() -> u32 {
    1024
}

// =============================================================================
// Orchestrator Profile Types
// =============================================================================

/// Orchestrator mode for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrchestratorModeResponse {
    Local,
    Remote,
}

impl From<config::OrchestratorMode> for OrchestratorModeResponse {
    fn from(mode: config::OrchestratorMode) -> Self {
        match mode {
            config::OrchestratorMode::Local => Self::Local,
            config::OrchestratorMode::Remote => Self::Remote,
        }
    }
}

impl From<OrchestratorModeResponse> for config::OrchestratorMode {
    fn from(mode: OrchestratorModeResponse) -> Self {
        match mode {
            OrchestratorModeResponse::Local => Self::Local,
            OrchestratorModeResponse::Remote => Self::Remote,
        }
    }
}

/// Orchestrator profile for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorProfileResponse {
    pub mode: OrchestratorModeResponse,
    pub url: Option<String>,
    /// API key is masked for display (only shows first/last 4 chars)
    pub api_key: Option<String>,
    /// How workers access the orchestrator
    pub access: config::OrchestratorAccess,
}

impl From<config::OrchestratorProfile> for OrchestratorProfileResponse {
    fn from(profile: config::OrchestratorProfile) -> Self {
        // Mask OAuth credentials in the access field
        let access = match profile.access {
            config::OrchestratorAccess::Direct => config::OrchestratorAccess::Direct,
            config::OrchestratorAccess::Tailscale {
                oauth_client_id,
                oauth_client_secret,
                tag,
            } => config::OrchestratorAccess::Tailscale {
                // Mask credentials - show first/last 4 chars
                oauth_client_id: if oauth_client_id.len() > 8 {
                    format!(
                        "{}...{}",
                        &oauth_client_id[..4],
                        &oauth_client_id[oauth_client_id.len() - 4..]
                    )
                } else {
                    "****".to_string()
                },
                oauth_client_secret: if oauth_client_secret.len() > 8 {
                    format!(
                        "{}...{}",
                        &oauth_client_secret[..4],
                        &oauth_client_secret[oauth_client_secret.len() - 4..]
                    )
                } else {
                    "****".to_string()
                },
                tag,
            },
        };

        Self {
            mode: profile.mode.into(),
            url: profile.url,
            api_key: profile.api_key.map(|k| {
                if k.len() > 8 {
                    format!("{}...{}", &k[..4], &k[k.len() - 4..])
                } else {
                    "****".to_string()
                }
            }),
            access,
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
    Tigris,
}

impl From<config::StorageProvider> for StorageProviderResponse {
    fn from(provider: config::StorageProvider) -> Self {
        match provider {
            config::StorageProvider::S3 => Self::S3,
            config::StorageProvider::Tigris => Self::Tigris,
        }
    }
}

impl From<StorageProviderResponse> for config::StorageProvider {
    fn from(provider: StorageProviderResponse) -> Self {
        match provider {
            StorageProviderResponse::S3 => Self::S3,
            StorageProviderResponse::Tigris => Self::Tigris,
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
    pub max_iterations: Option<u32>,
    pub user_message_pause: String,
    pub human_in_the_loop: bool,
    pub compaction_enabled: bool,
    pub compaction_threshold: Option<u32>,
    pub compaction_keep_messages: u32,
    pub auto_improve: bool,
    pub context_warning_threshold: f64,
    pub coordinator_port: u16,
    pub auth: AuthConfigResponse,
    pub runners: std::collections::HashMap<String, RunnerConfigResponse>,
    pub default_runner: Option<String>,
    pub worker_runners: std::collections::HashMap<String, String>,
    pub profiles: std::collections::HashMap<String, OrchestratorProfileResponse>,
    pub default_profile: String,
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
pub fn convert_status(status: crate::core::state::Status) -> RunStatus {
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

/// Check if status represents a completed run
pub fn is_completed_status(status: &RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Done | RunStatus::Failed | RunStatus::Delivered
    )
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

/// Calculate duration in minutes between two RFC3339 timestamps
pub fn calculate_duration_minutes(start: &str, end: &str) -> f64 {
    if let (Ok(start_ts), Ok(end_ts)) = (
        chrono::DateTime::parse_from_rfc3339(start),
        chrono::DateTime::parse_from_rfc3339(end),
    ) {
        let duration = end_ts.signed_duration_since(start_ts);
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
    pub max_iterations: Option<Option<u32>>,
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

/// Request to update compaction configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionConfigRequest {
    pub enabled: Option<bool>,
    pub threshold: Option<Option<u32>>,
    pub keep_messages: Option<u32>,
}

/// Request to update agent auth configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthConfigRequest {
    pub method: AuthMethodResponse,
    pub api_key: Option<String>,
    pub env_var: Option<String>,
}

impl From<AgentAuthConfigRequest> for config::AgentAuth {
    fn from(req: AgentAuthConfigRequest) -> Self {
        Self {
            method: req.method.into(),
            api_key: req.api_key,
            env_var: req.env_var,
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
