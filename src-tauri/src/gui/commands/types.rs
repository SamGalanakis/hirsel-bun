//! GUI-specific types for Tauri commands
//!
//! Types that are only needed by the GUI frontend, not shared with
//! the orchestrator or server.

use serde::{Deserialize, Serialize};

use crate::core::api_types::{GitProviderResponse, LlmProviderResponse, RunnerConfigResponse};
use crate::core::config;

// =============================================================================
// GUI-Specific Types (not needed by orchestrator/server)
// =============================================================================

/// Unread worker concern notification aggregated across projects/routes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadNotification {
    pub id: String,
    pub concern_id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub route_id: i64,
    pub route_name: String,
    pub worker_name: String,
    pub kind: String,
    pub severity: String,
    pub summary: String,
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

/// LLM config update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfigUpdate {
    pub provider: Option<LlmProviderResponse>,
    pub openrouter_base_url: Option<Option<String>>,
}

impl LlmConfigUpdate {
    pub fn apply(self, target: &mut config::LlmConfig) {
        if let Some(provider) = self.provider {
            target.provider = provider.into();
        }
        if let Some(base_url) = self.openrouter_base_url {
            target.openrouter_base_url = base_url.and_then(|v| {
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

/// Backend connection update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendConfigUpdate {
    pub url: Option<String>,
    pub api_key: Option<String>,
}

impl From<BackendConfigUpdate> for config::BackendConfig {
    fn from(update: BackendConfigUpdate) -> Self {
        Self {
            url: update.url,
            api_key: update.api_key,
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
    pub human_in_the_loop: Option<bool>,
    pub context_warning_threshold: Option<f64>,
    pub coordinator_port: Option<u16>,
    pub llm: Option<LlmConfigUpdate>,
    pub runners: Option<std::collections::HashMap<String, RunnerConfigResponse>>,
    pub default_runner: Option<Option<String>>,
    pub worker_runners: Option<std::collections::HashMap<String, String>>,
    pub backend: Option<BackendConfigUpdate>,
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

/// Parsed log line
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedLogLine {
    pub line_type: String,
    pub content: String,
    pub tool_name: Option<String>,
    pub tool_input: Option<String>,
}

/// Shepherd chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdChatMessage {
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

/// Config defaults for project settings inheritance
///
/// These values are used as defaults when project-specific settings are not set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDefaults {
    /// Default worker scale (typically "5")
    pub worker_scale: String,
    /// Default time limit in minutes (None = no limit)
    pub time_limit_minutes: Option<i64>,
    /// Default human-in-the-loop setting
    pub human_in_the_loop: bool,
    /// Available runner names from global config
    pub runners: Vec<String>,
    /// Default runner name from global config
    pub default_runner: Option<String>,
}
