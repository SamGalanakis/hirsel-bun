//! Shared types for GUI commands
//!
//! This module re-exports types from core::api_types and provides
//! additional GUI-specific types.

// Re-export all types from core::api_types for backward compatibility
pub use crate::core::api_types::{
    // Helper functions
    calculate_duration_minutes,
    convert_status,
    is_completed_status,
    parse_elapsed_minutes,
    parse_timestamp,
    // Auth types
    AgentAuthResponse,
    AuthConfigResponse,
    AuthMethodResponse,
    // Main types
    ConfigResponse,
    Eval,
    // Status enums
    EvalStatus,
    // Git provider types
    GitConfigResponse,
    GitProviderResponse,
    HistoryEntry,
    Message,
    // Orchestrator profile types
    OrchestratorModeResponse,
    OrchestratorProfileResponse,
    // Runner types
    RunDetail,
    RunStatus,
    RunSummary,
    RunnerConfigResponse,
    SheepConfig,
    Task,
    TaskStatus,
    ThreadSummary,
    Worker,
    // Worker event types
    WorkerEventResponse,
    WorkerEventsResponse,
    WorkerLocation,
    WorkerStatus,
};

use serde::{Deserialize, Serialize};

use crate::core::config;

// =============================================================================
// GUI-Specific Types (not needed by orchestrator/server)
// =============================================================================

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

/// Agent preset configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPreset {
    pub name: String,
    pub command: Vec<String>,
    pub mcp_config: Option<serde_json::Value>,
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

/// Orchestrator profile update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorProfileUpdate {
    pub mode: crate::core::api_types::OrchestratorModeResponse,
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub access: config::OrchestratorAccess,
}

impl From<OrchestratorProfileUpdate> for config::OrchestratorProfile {
    fn from(update: OrchestratorProfileUpdate) -> Self {
        Self {
            mode: update.mode.into(),
            url: update.url,
            api_key: update.api_key,
            access: update.access,
        }
    }
}

/// Git configuration update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitConfigUpdate {
    pub default_provider: Option<GitProviderResponse>,
}

impl From<GitConfigUpdate> for config::GitConfig {
    fn from(update: GitConfigUpdate) -> Self {
        Self {
            default_provider: update.default_provider.map(|p| p.into()),
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
    pub context_warning_threshold: Option<f64>,
    pub coordinator_port: Option<u16>,
    pub auth: Option<AuthConfigUpdate>,
    pub runners: Option<std::collections::HashMap<String, RunnerConfigResponse>>,
    pub default_runner: Option<Option<String>>,
    pub worker_runners: Option<std::collections::HashMap<String, String>>,
    pub profiles: Option<std::collections::HashMap<String, OrchestratorProfileUpdate>>,
    pub default_profile: Option<String>,
    pub git: Option<GitConfigUpdate>,
    pub storage: Option<crate::core::api_types::StorageConfigResponse>,
}

/// Result of validating a repository path/URL
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoValidation {
    pub valid: bool,
    pub error: Option<String>,
    pub is_remote: bool,
    pub branches: Vec<String>,
    pub current_branch: Option<String>,
    pub repo_url: String,
    pub url_branch: Option<String>,
    pub url_branch_valid: bool,
    pub needs_dir_create: bool,
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
    pub runner: Option<String>,
    pub worker_runners: Option<std::collections::HashMap<String, String>>,
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
