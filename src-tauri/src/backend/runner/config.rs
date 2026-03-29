//! Runner configuration for single-host worker execution.

use serde::{Deserialize, Serialize};

/// Container configuration for running workers in Docker.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContainerConfig {
    /// Docker image URI (for example, "debian:bookworm-slim").
    pub image: String,
}

/// Runner configuration for worker execution on the backend host.
///
/// Workers always run on the coordinator machine. The only choice is whether
/// they run directly on the host or inside a local container.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunnerConfig {
    /// Optional container configuration for local containerized execution.
    #[serde(default)]
    pub container: Option<ContainerConfig>,
}

impl RunnerConfig {
    /// Create a host-local runner config with no container.
    pub fn local() -> Self {
        Self { container: None }
    }

    /// Create a host-local runner config with a container image.
    pub fn local_docker(image: String) -> Self {
        Self {
            container: Some(ContainerConfig { image }),
        }
    }

    /// Check if this runner uses a container.
    pub fn uses_container(&self) -> bool {
        self.container.is_some()
    }

    /// Human-readable execution kind for logs and UI.
    pub fn execution_kind(&self) -> &'static str {
        if self.uses_container() {
            "docker"
        } else {
            "local"
        }
    }
}
