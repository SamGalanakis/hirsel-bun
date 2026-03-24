//! LLM provider configuration.

use serde::{Deserialize, Serialize};

/// Supported LLM providers for lash runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    Codex,
    Openrouter,
}

impl Default for LlmProvider {
    fn default() -> Self {
        Self::Codex
    }
}

/// Per-tier model overrides for agent intelligence levels.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelOverrides {
    /// Fast/cheap model for exploration and read-only tasks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low: Option<String>,
    /// Balanced model for bounded implementation work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub medium: Option<String>,
    /// Capable model for complex, peer-level work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high: Option<String>,
}

/// Global LLM configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfig {
    /// Active provider used for Shepherd and worker runtimes.
    #[serde(default)]
    pub provider: LlmProvider,
    /// Optional OpenRouter-compatible base URL.
    #[serde(default)]
    pub openrouter_base_url: Option<String>,
    /// Override the main model (Shepherd + route workers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Model reasoning variant (e.g. "low", "medium", "high", "xhigh").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_variant: Option<String>,
    /// Per-tier model overrides for delegated agent calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_models: Option<AgentModelOverrides>,
}
