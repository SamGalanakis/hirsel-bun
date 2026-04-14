//! Shared model/provider configuration types.

use serde::{Deserialize, Serialize};

/// Supported LLM providers for lash runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    #[default]
    Codex,
    Openrouter,
}

/// Optional model override for a specific Hirsel runtime role.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RoleModelConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_variant: Option<String>,
}

impl RoleModelConfig {
    pub fn is_empty(&self) -> bool {
        self.model.is_none() && self.model_variant.is_none()
    }
}

/// Per-role model overrides for Hirsel's long-lived agents.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RoleModelOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shepherd: Option<RoleModelConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub librarian: Option<RoleModelConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<RoleModelConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<RoleModelConfig>,
}
