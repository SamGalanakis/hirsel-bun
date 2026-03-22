//! Backend connection configuration.

use serde::{Deserialize, Serialize};

/// Backend connection settings for a Hirsel client.
///
/// When `url` is set, clients talk to the remote Hirsel backend over HTTP.
/// When `url` is absent, the local desktop app falls back to its embedded
/// local runtime. That fallback is an internal transport detail, not a
/// user-facing profile model.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BackendConfig {
    /// Base URL for the Hirsel backend.
    pub url: Option<String>,
    /// Optional API key for authenticated backend access.
    pub api_key: Option<String>,
}
