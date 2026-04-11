//! Backend connection configuration.

use serde::{Deserialize, Serialize};

/// Backend connection settings for a Hirsel client.
///
/// Clients talk to a Hirsel backend over HTTP. The desktop or mobile app is
/// just a client shell and does not own project execution locally.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BackendConfig {
    /// Base URL for the Hirsel backend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Optional API key for authenticated backend access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}
