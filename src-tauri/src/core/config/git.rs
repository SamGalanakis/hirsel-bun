//! Git provider configuration.

use serde::{Deserialize, Serialize};

/// Git provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitProvider {
    Github,
    // Future: Gitlab, Bitbucket, etc.
}

impl std::fmt::Display for GitProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Github => write!(f, "github"),
        }
    }
}

/// Git provider configuration
///
/// Tokens are stored in CredentialStore, not in config.
/// This struct only tracks which providers are configured.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitConfig {
    /// Default git provider to use
    pub default_provider: Option<GitProvider>,
}
