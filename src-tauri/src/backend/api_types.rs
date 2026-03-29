//! Small shared API types used by the desktop shell and backend settings UI.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::backend::config;

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelOverridesResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub medium: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfigResponse {
    pub provider: LlmProviderResponse,
    pub openrouter_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_models: Option<AgentModelOverridesResponse>,
}

impl From<config::LlmConfig> for LlmConfigResponse {
    fn from(value: config::LlmConfig) -> Self {
        Self {
            provider: value.provider.into(),
            openrouter_base_url: value.openrouter_base_url,
            model: value.model,
            model_variant: value.model_variant,
            agent_models: value.agent_models.map(|am| AgentModelOverridesResponse {
                low: am.low,
                medium: am.medium,
                high: am.high,
            }),
        }
    }
}

impl From<LlmConfigResponse> for config::LlmConfig {
    fn from(value: LlmConfigResponse) -> Self {
        Self {
            provider: value.provider.into(),
            openrouter_base_url: value.openrouter_base_url,
            model: value.model,
            model_variant: value.model_variant,
            agent_models: value.agent_models.map(|am| config::AgentModelOverrides {
                low: am.low,
                medium: am.medium,
                high: am.high,
            }),
        }
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigResponse {
    pub llm: LlmConfigResponse,
    pub backend: BackendConfigResponse,
    pub mcp_servers: BTreeMap<String, config::McpServerConfig>,
}

pub fn mask_credential(value: &str) -> String {
    if value.len() > 8 {
        format!("{}...{}", &value[..4], &value[value.len() - 4..])
    } else {
        "****".to_string()
    }
}
