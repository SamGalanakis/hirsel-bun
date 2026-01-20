//! Orchestrator mode and profile configuration.

use serde::{Deserialize, Serialize};

/// Orchestrator mode - local or remote
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OrchestratorMode {
    #[default]
    Local,
    Remote,
}

/// How workers access the orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OrchestratorAccess {
    /// Direct access - assumes network is already configured (VPC, same network, etc.)
    Direct,
    /// Tailscale - workers join the user's tailnet via OAuth-generated auth keys
    Tailscale {
        /// OAuth client ID from Tailscale admin console
        oauth_client_id: String,
        /// OAuth client secret from Tailscale admin console
        oauth_client_secret: String,
        /// Optional tag to apply to worker devices (e.g., "tag:hirsel-worker")
        #[serde(default)]
        tag: Option<String>,
    },
}

impl Default for OrchestratorAccess {
    fn default() -> Self {
        Self::Direct
    }
}

/// Orchestrator profile configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorProfile {
    #[serde(default)]
    pub mode: OrchestratorMode,
    /// Server URL for remote mode
    pub url: Option<String>,
    /// API key for remote mode
    pub api_key: Option<String>,
    /// How workers access the orchestrator (network strategy)
    #[serde(default)]
    pub access: OrchestratorAccess,
}

impl Default for OrchestratorProfile {
    fn default() -> Self {
        Self {
            mode: OrchestratorMode::Local,
            url: None,
            api_key: None,
            access: OrchestratorAccess::Direct,
        }
    }
}

impl OrchestratorProfile {
    /// Get Tailscale OAuth credentials if access is configured for Tailscale
    pub fn tailscale_oauth(&self) -> Option<(&str, &str, Option<&str>)> {
        match &self.access {
            OrchestratorAccess::Tailscale {
                oauth_client_id,
                oauth_client_secret,
                tag,
            } => Some((
                oauth_client_id.as_str(),
                oauth_client_secret.as_str(),
                tag.as_deref(),
            )),
            _ => None,
        }
    }
}
