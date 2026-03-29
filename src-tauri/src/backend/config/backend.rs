//! Backend connection configuration.

use serde::{Deserialize, Serialize};

/// Backend connection settings for a Hirsel client.
///
/// Clients talk to a Hirsel backend over HTTP. The desktop or mobile app is
/// just a client shell and does not own project execution locally.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BackendConfig {
    /// Base URL for the Hirsel backend.
    pub url: Option<String>,
    /// Optional API key for authenticated backend access.
    pub api_key: Option<String>,
}
