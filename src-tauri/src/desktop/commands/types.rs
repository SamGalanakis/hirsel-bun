//! GUI-specific types for Tauri commands
//!
//! Types that are only needed by the GUI frontend, not shared with
//! the orchestrator or server.

use serde::{Deserialize, Serialize};

use crate::backend::api_types::LlmProviderResponse;
use crate::backend::config;

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

/// Agent model overrides for update requests
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelOverridesUpdate {
    #[serde(default)]
    pub low: Option<String>,
    #[serde(default)]
    pub medium: Option<String>,
    #[serde(default)]
    pub high: Option<String>,
}

/// LLM config update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfigUpdate {
    pub provider: Option<LlmProviderResponse>,
    pub openrouter_base_url: Option<Option<String>>,
    #[serde(default)]
    pub model: Option<Option<String>>,
    #[serde(default)]
    pub model_variant: Option<Option<String>>,
    #[serde(default)]
    pub agent_models: Option<Option<AgentModelOverridesUpdate>>,
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
        if let Some(model) = self.model {
            target.model = model.and_then(|v| {
                let trimmed = v.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            });
        }
        if let Some(variant) = self.model_variant {
            target.model_variant = variant.and_then(|v| {
                let trimmed = v.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            });
        }
        if let Some(agent_models) = self.agent_models {
            target.agent_models = agent_models.map(|am| {
                let clean = |v: Option<String>| {
                    v.and_then(|s| {
                        let t = s.trim();
                        if t.is_empty() {
                            None
                        } else {
                            Some(t.to_string())
                        }
                    })
                };
                config::AgentModelOverrides {
                    low: clean(am.low),
                    medium: clean(am.medium),
                    high: clean(am.high),
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

/// Request to update configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigUpdateRequest {
    pub llm: Option<LlmConfigUpdate>,
    pub backend: Option<BackendConfigUpdate>,
    pub mcp_servers: Option<std::collections::BTreeMap<String, config::McpServerConfig>>,
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
