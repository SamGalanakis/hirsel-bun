//! Shared types for GUI commands
//!
//! This module contains all the types used across GUI command modules,
//! matching TypeScript definitions in src/lib/types.ts.

use serde::{Deserialize, Serialize};

use crate::core::config;

// =============================================================================
// Status Enums (match TypeScript types)
// =============================================================================

/// Run status values matching TypeScript RunStatus
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Draft,
    Idle,
    Working,
    Paused,
    Runaway,
    TimedOut,
    Eval,
    EvalFailed,
    Waiting,
    Done,
    Delivered,
    Merged,
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
    Idle,
    Working,
    Waiting,
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
// Response Types (match TypeScript interfaces)
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
    // Additional fields for status bar display
    pub tasks_done: u32,
    pub tasks_total: u32,
    pub workers_active: u32,
    pub workers_total: u32,
    pub elapsed_minutes: f64,
    // Learnings and compaction status
    pub learnings_count: u32,
    pub learnings_processed_at: Option<String>,
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

/// Sheep avatar configuration - deterministically generated from worker name
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheepConfig {
    /// Hat type (0=none, 1=crown, 2=cowboy, 3=tophat, 4=beanie, 5=wizard, 6=chef, 7=hardhat)
    pub hat: u8,
    /// Wool fluffiness (0-3, affects wool layer count/opacity)
    pub fluffiness: u8,
    /// Body width modifier (-2 to +2)
    pub body_width: i8,
    /// Body height modifier (-2 to +2)
    pub body_height: i8,
    /// Ear position modifier (-1 to +1 for forward/back positioning)
    pub ear_position: i8,
    /// Leg length modifier (-1 to +1)
    pub leg_length: i8,
    /// Wool color hue shift (0-359 degrees, applied as CSS filter)
    pub hue_shift: u16,
    /// Glasses type (0=none, 1=round, 2=square, 3=sunglasses, 4=eyepatch)
    pub glasses: u8,
    /// Bow tie (0=none, 1=red, 2=blue, 3=gold, 4=pink)
    pub bowtie: u8,
}

impl SheepConfig {
    /// Generate deterministic config from worker name
    /// Hat 8 (detective) is reserved for eval agents
    pub fn from_name(name: &str, is_leader: bool) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        let hash = hasher.finish();

        // Use different parts of the hash for different attributes
        let bytes = hash.to_le_bytes();

        SheepConfig {
            // Leaders always get crown (1), others get random hat (0-7, where 0=none)
            // Hat 8 (detective) is reserved for eval agents
            hat: if is_leader { 1 } else { bytes[0] % 8 },
            fluffiness: bytes[1] % 4,                 // 0-3
            body_width: ((bytes[2] % 5) as i8) - 2,   // -2 to +2
            body_height: ((bytes[3] % 5) as i8) - 2,  // -2 to +2
            ear_position: ((bytes[4] % 3) as i8) - 1, // -1 to +1
            leg_length: ((bytes[5] % 3) as i8) - 1,   // -1 to +1
            hue_shift: 0,                             // disabled - looks odd
            glasses: bytes[6] % 5,                    // 0-4 (0=none most common)
            bowtie: bytes[7] % 5,                     // 0-4 (0=none most common)
        }
    }

    /// Generate deterministic config for an eval agent
    /// Always uses detective hat (8), unique appearance based on eval id
    pub fn for_eval(eval_id: u32) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        eval_id.hash(&mut hasher);
        let hash = hasher.finish();

        let bytes = hash.to_le_bytes();

        SheepConfig {
            hat: 8,                                   // Always detective hat
            fluffiness: bytes[1] % 4,                 // 0-3
            body_width: ((bytes[2] % 5) as i8) - 2,   // -2 to +2
            body_height: ((bytes[3] % 5) as i8) - 2,  // -2 to +2
            ear_position: ((bytes[4] % 3) as i8) - 1, // -1 to +1
            leg_length: ((bytes[5] % 3) as i8) - 1,   // -1 to +1
            hue_shift: 0,                             // disabled
            glasses: bytes[6] % 5,                    // 0-4 (0=none most common)
            bowtie: bytes[7] % 5,                     // 0-4 (0=none most common)
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
    // Display fields
    pub is_leader: bool,
    pub context_utilization: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub turns: Option<u32>,
    pub current_task: Option<String>,
    /// Sheep avatar configuration
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

/// Unread notification aggregated across all runs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadNotification {
    pub id: String,
    pub run_name: String,
    pub thread: String,
    pub sender: String,
    pub content: String,
    pub timestamp: String,
}

/// Response for get_all_unread_notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadNotificationsResponse {
    pub notifications: Vec<UnreadNotification>,
    pub total_runs_with_unread: u32,
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
    /// Sheep avatar configuration (detective hat)
    pub sheep_config: SheepConfig,
}

/// Agent preset configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPreset {
    pub name: String,
    pub command: Vec<String>,
    pub mcp_config: Option<serde_json::Value>,
}

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
            // Don't expose full API key, just indicate if one is set
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

/// Remote configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConfigResponse {
    pub host: String,
    pub ssh_key: Option<String>,
    pub ssh_port: u16,
    pub work_base: String,
    pub python_path: String,
    pub location: Option<String>,
}

impl From<config::RemoteConfig> for RemoteConfigResponse {
    fn from(remote: config::RemoteConfig) -> Self {
        Self {
            host: remote.host,
            ssh_key: remote.ssh_key,
            ssh_port: remote.ssh_port,
            work_base: remote.work_base,
            python_path: remote.python_path,
            location: remote.location,
        }
    }
}

/// Runner configuration for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RunnerConfigResponse {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "ssh")]
    Ssh {
        host: String,
        ssh_key: Option<String>,
        ssh_port: u16,
        work_base: String,
        location: Option<String>,
    },
    #[serde(rename = "sprite")]
    Sprite {
        api_token: Option<String>,
        base_checkpoint: Option<String>,
        #[serde(default = "default_auto_destroy")]
        auto_destroy: bool,
        #[serde(default = "default_idle_timeout_secs")]
        idle_timeout_secs: u32,
        #[serde(default = "default_api_url")]
        api_url: String,
    },
}

impl From<crate::core::runner::RunnerConfig> for RunnerConfigResponse {
    fn from(cfg: crate::core::runner::RunnerConfig) -> Self {
        match cfg {
            crate::core::runner::RunnerConfig::Local => RunnerConfigResponse::Local,
            crate::core::runner::RunnerConfig::Ssh(ssh) => RunnerConfigResponse::Ssh {
                host: ssh.host,
                ssh_key: ssh.ssh_key,
                ssh_port: ssh.ssh_port,
                work_base: ssh.work_base,
                location: ssh.location,
            },
            crate::core::runner::RunnerConfig::Sprite(sprite) => RunnerConfigResponse::Sprite {
                api_token: sprite.api_token,
                base_checkpoint: sprite.base_checkpoint,
                auto_destroy: sprite.auto_destroy,
                idle_timeout_secs: sprite.idle_timeout_secs,
                api_url: sprite.api_url,
            },
        }
    }
}

impl From<RunnerConfigResponse> for crate::core::runner::RunnerConfig {
    fn from(cfg: RunnerConfigResponse) -> Self {
        match cfg {
            RunnerConfigResponse::Local => crate::core::runner::RunnerConfig::Local,
            RunnerConfigResponse::Ssh {
                host,
                ssh_key,
                ssh_port,
                work_base,
                location,
            } => crate::core::runner::RunnerConfig::Ssh(crate::core::runner::SshRunnerConfig {
                host,
                ssh_key,
                ssh_port,
                work_base,
                location,
            }),
            RunnerConfigResponse::Sprite {
                api_token,
                base_checkpoint,
                auto_destroy,
                idle_timeout_secs,
                api_url,
            } => {
                crate::core::runner::RunnerConfig::Sprite(crate::core::runner::SpriteRunnerConfig {
                    api_token,
                    base_checkpoint,
                    auto_destroy,
                    idle_timeout_secs,
                    api_url,
                })
            }
        }
    }
}

// Default functions for sprite runner fields
fn default_auto_destroy() -> bool {
    true
}
fn default_idle_timeout_secs() -> u32 {
    30
}
fn default_api_url() -> String {
    "https://api.sprites.dev".to_string()
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
    pub remotes: std::collections::HashMap<String, RemoteConfigResponse>,
    pub default_remote: Option<String>,
    pub runners: std::collections::HashMap<String, RunnerConfigResponse>,
    pub default_runner: Option<String>,
    pub worker_runners: std::collections::HashMap<String, String>,
}

/// Agent auth update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthUpdate {
    pub method: AuthMethodResponse,
    pub api_key: Option<String>,
    pub env_var: Option<String>,
}

impl From<AgentAuthUpdate> for config::AgentAuth {
    fn from(update: AgentAuthUpdate) -> Self {
        Self {
            method: update.method.into(),
            api_key: update.api_key,
            env_var: update.env_var,
        }
    }
}

/// Auth config update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfigUpdate {
    pub default_method: Option<AuthMethodResponse>,
    pub claude: Option<AgentAuthUpdate>,
    pub gemini: Option<AgentAuthUpdate>,
    pub codex: Option<AgentAuthUpdate>,
    pub goose: Option<AgentAuthUpdate>,
}

/// Remote config update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConfigUpdate {
    pub host: String,
    pub ssh_key: Option<String>,
    pub ssh_port: Option<u16>,
    pub work_base: Option<String>,
    pub python_path: Option<String>,
    pub location: Option<String>,
}

impl From<RemoteConfigUpdate> for config::RemoteConfig {
    fn from(update: RemoteConfigUpdate) -> Self {
        Self {
            host: update.host,
            ssh_key: update.ssh_key,
            ssh_port: update.ssh_port.unwrap_or(22),
            work_base: update
                .work_base
                .unwrap_or_else(|| "/tmp/hirsel-remote".to_string()),
            python_path: update.python_path.unwrap_or_else(|| "python3".to_string()),
            location: update.location,
        }
    }
}

/// Request to update configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigUpdateRequest {
    pub agent_command: Option<Vec<String>>,
    pub eval_timeout: Option<u32>,
    pub auto_learn: Option<bool>,
    pub max_iterations: Option<Option<u32>>,
    pub user_message_pause: Option<String>,
    pub human_in_the_loop: Option<bool>,
    pub compaction_enabled: Option<bool>,
    pub compaction_threshold: Option<Option<u32>>,
    pub compaction_keep_messages: Option<u32>,
    pub auto_improve: Option<bool>,
    pub context_warning_threshold: Option<f64>,
    pub coordinator_port: Option<u16>,
    pub auth: Option<AuthConfigUpdate>,
    pub remotes: Option<std::collections::HashMap<String, RemoteConfigUpdate>>,
    pub default_remote: Option<Option<String>>,
    pub runners: Option<std::collections::HashMap<String, RunnerConfigResponse>>,
    pub default_runner: Option<Option<String>>,
    pub worker_runners: Option<std::collections::HashMap<String, String>>,
}

/// Result of validating a repository path/URL
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoValidation {
    /// Whether the repo is valid and accessible
    pub valid: bool,
    /// Error message if not valid
    pub error: Option<String>,
    /// Whether this is a remote URL (vs local path)
    pub is_remote: bool,
    /// Available branches in the repository
    pub branches: Vec<String>,
    /// Currently checked out branch (for local repos)
    pub current_branch: Option<String>,
    /// The normalized repo URL (with branch stripped if it was in the URL)
    pub repo_url: String,
    /// Branch extracted from URL (if any)
    pub url_branch: Option<String>,
    /// Whether the URL branch exists in the repo
    pub url_branch_valid: bool,
    /// Whether the directory needs to be created (local paths only)
    pub needs_dir_create: bool,
    /// Whether git needs to be initialized (local paths only)
    pub needs_git_init: bool,
}

/// Draft update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftUpdateRequest {
    pub spec: Option<String>,
    pub worker_scale: Option<String>,
    pub time_limit_minutes: Option<i64>,
    pub human_in_the_loop: Option<bool>,
    pub project_path: Option<String>,
    pub branch: Option<String>,
    pub name: Option<String>,
}

/// Worker event response
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

/// Worker events response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEventsResponse {
    pub events: Vec<WorkerEventResponse>,
    pub last_id: Option<i64>,
    pub worker_status: Option<String>,
}

/// Parsed log line
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedLogLine {
    pub line_type: String,
    pub content: String,
    pub tool_name: Option<String>,
    pub tool_input: Option<String>,
}

/// Gyp chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GypChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}
