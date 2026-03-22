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
}
