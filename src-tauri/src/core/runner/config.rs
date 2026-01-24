//! Runner configuration types for Host + Container model.

use serde::{Deserialize, Serialize};

use super::types::{OrchestratorMode, RunnerError, RunnerResult};

// =============================================================================
// Default value functions
// =============================================================================

fn default_ssh_port() -> u16 {
    22
}

fn default_work_base() -> String {
    "/tmp/hirsel-remote".to_string()
}

fn default_auto_destroy() -> bool {
    true
}

fn default_idle_timeout() -> u32 {
    30
}

fn default_api_url() -> String {
    "https://api.sprites.dev".to_string()
}

fn default_fly_cpu_kind() -> String {
    "shared".to_string()
}

fn default_fly_cpus() -> u32 {
    1
}

fn default_fly_memory_mb() -> u32 {
    1024
}

// =============================================================================
// Container Configuration
// =============================================================================

/// Container configuration for running workers in Docker.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContainerConfig {
    /// Docker image URI (e.g., "rust:latest", "ghcr.io/org/dev-env")
    pub image: String,
}

// =============================================================================
// Host Configurations
// =============================================================================

/// SSH host configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshHostConfig {
    /// SSH address (e.g., "user@server.example.com")
    pub address: String,
    /// SSH port (default: 22)
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// Path to SSH private key (optional)
    #[serde(default)]
    pub ssh_key: Option<String>,
    /// Base directory for work on remote
    #[serde(default = "default_work_base")]
    pub work_base: String,
    /// Display name for location
    #[serde(default)]
    pub location: Option<String>,
}

impl Default for SshHostConfig {
    fn default() -> Self {
        Self {
            address: String::new(),
            port: default_ssh_port(),
            ssh_key: None,
            work_base: default_work_base(),
            location: None,
        }
    }
}

/// Sprite host configuration for Sprites.dev cloud VMs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteHostConfig {
    /// Sprites API token (stored directly)
    #[serde(default)]
    pub api_token: Option<String>,
    /// Base checkpoint to clone from (pre-configured image)
    #[serde(default)]
    pub checkpoint: Option<String>,
    /// Auto-destroy sprite when worker completes
    #[serde(default = "default_auto_destroy")]
    pub auto_destroy: bool,
    /// Max idle time before sleep (sprites auto-hibernate at 30s anyway)
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u32,
    /// Sprites API base URL
    #[serde(default = "default_api_url")]
    pub api_url: String,
    /// Use push mode to send files directly to worker via HTTP.
    /// When enabled, the worker starts a file receiver server (port 19800)
    /// and files are pushed from the coordinator. Requires Tailscale
    /// connectivity between coordinator and sprites.
    #[serde(default)]
    pub use_file_push: bool,
}

impl Default for SpriteHostConfig {
    fn default() -> Self {
        Self {
            api_token: None,
            checkpoint: None,
            auto_destroy: default_auto_destroy(),
            idle_timeout_secs: default_idle_timeout(),
            api_url: default_api_url(),
            use_file_push: false,
        }
    }
}

/// Fly.io host configuration for ephemeral Fly Machines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlyHostConfig {
    /// Fly.io API token (or use FLY_API_TOKEN env var)
    #[serde(default)]
    pub api_token: Option<String>,
    /// Fly app name (machines are created under this app)
    pub app: String,
    /// Region (default: nearest, e.g., "sjc", "iad")
    #[serde(default)]
    pub region: Option<String>,
    /// CPU kind: "shared" or "performance" (default: "shared")
    #[serde(default = "default_fly_cpu_kind")]
    pub cpu_kind: String,
    /// Number of CPUs (default: 1)
    #[serde(default = "default_fly_cpus")]
    pub cpus: u32,
    /// Memory in MB (default: 1024)
    #[serde(default = "default_fly_memory_mb")]
    pub memory_mb: u32,
    /// Auto-destroy machine when worker completes (default: true)
    #[serde(default = "default_auto_destroy")]
    pub auto_destroy: bool,
}

impl Default for FlyHostConfig {
    fn default() -> Self {
        Self {
            api_token: None,
            app: String::new(),
            region: None,
            cpu_kind: default_fly_cpu_kind(),
            cpus: default_fly_cpus(),
            memory_mb: default_fly_memory_mb(),
            auto_destroy: default_auto_destroy(),
        }
    }
}

// =============================================================================
// Host Configuration Enum
// =============================================================================

/// Host configuration - defines where compute runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[derive(Default)]
pub enum HostConfig {
    /// Local host - run on this machine
    #[default]
    Local,
    /// Client host - (remote mode only) SSH back to GUI/CLI machine via Tailscale
    Client,
    /// SSH host - run on remote machine via SSH
    Ssh(SshHostConfig),
    /// Sprite host - run on Sprites.dev cloud VM
    Sprite(SpriteHostConfig),
    /// Fly host - run on Fly.io ephemeral machines
    Fly(FlyHostConfig),
}

impl HostConfig {
    /// Check if this host type is compatible with the given orchestrator mode.
    pub fn is_compatible_with(&self, mode: OrchestratorMode) -> bool {
        match mode {
            OrchestratorMode::Local => {
                // Local orchestrator: Local and SSH work
                // - Local: direct SQLite access
                // - SSH: reverse tunnel to daemon TCP on localhost:19700
                // Other hosts need a publicly accessible coordinator URL
                matches!(self, HostConfig::Local | HostConfig::Ssh(_))
            }
            OrchestratorMode::Remote => {
                // Remote orchestrator: all hosts work (they connect via HTTP)
                true
            }
        }
    }

    /// Get the reason why this host is incompatible with local mode.
    pub fn local_incompatibility_reason(&self) -> Option<&'static str> {
        match self {
            HostConfig::Local => None,
            HostConfig::Ssh(_) => None, // SSH works via reverse tunnel to daemon TCP
            HostConfig::Sprite(_) => Some("Sprite runner requires publicly accessible HTTP coordinator (use `hirsel serve` or remote mode)"),
            HostConfig::Fly(_) => Some("Fly runner requires publicly accessible HTTP coordinator (use `hirsel serve` or remote mode)"),
            HostConfig::Client => Some("Client runner is only available in remote mode"),
        }
    }
}

// =============================================================================
// Host Configuration Shortcut
// =============================================================================

/// Shortcut or full host configuration.
/// Allows both `host = "local"` and `[host]` table in TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HostConfigOrShortcut {
    /// Simple string shortcut: "local" or "client"
    Shortcut(String),
    /// Full host configuration object
    Full(HostConfig),
}

impl HostConfigOrShortcut {
    /// Resolve the shortcut to a full HostConfig.
    pub fn resolve(&self) -> HostConfig {
        match self {
            HostConfigOrShortcut::Shortcut(s) => match s.to_lowercase().as_str() {
                "local" => HostConfig::Local,
                "client" => HostConfig::Client,
                _ => HostConfig::Local, // Default to local for unknown shortcuts
            },
            HostConfigOrShortcut::Full(config) => config.clone(),
        }
    }
}

impl Default for HostConfigOrShortcut {
    fn default() -> Self {
        HostConfigOrShortcut::Shortcut("local".to_string())
    }
}

// =============================================================================
// Runner Configuration
// =============================================================================

/// Runner configuration using Host + Container model.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunnerConfig {
    /// Host configuration (where compute runs)
    #[serde(default)]
    pub host: HostConfigOrShortcut,
    /// Optional container configuration (Docker)
    #[serde(default)]
    pub container: Option<ContainerConfig>,
}

impl RunnerConfig {
    /// Create a local runner config (bare host, no container).
    pub fn local() -> Self {
        RunnerConfig {
            host: HostConfigOrShortcut::Shortcut("local".to_string()),
            container: None,
        }
    }

    /// Create a local runner config with Docker container.
    pub fn local_docker(image: String) -> Self {
        RunnerConfig {
            host: HostConfigOrShortcut::Shortcut("local".to_string()),
            container: Some(ContainerConfig { image }),
        }
    }

    /// Create an SSH runner config.
    pub fn ssh(ssh_config: SshHostConfig) -> Self {
        RunnerConfig {
            host: HostConfigOrShortcut::Full(HostConfig::Ssh(ssh_config)),
            container: None,
        }
    }

    /// Create an SSH runner config with Docker container.
    pub fn ssh_docker(ssh_config: SshHostConfig, image: String) -> Self {
        RunnerConfig {
            host: HostConfigOrShortcut::Full(HostConfig::Ssh(ssh_config)),
            container: Some(ContainerConfig { image }),
        }
    }

    /// Create a Sprite runner config.
    pub fn sprite(sprite_config: SpriteHostConfig) -> Self {
        RunnerConfig {
            host: HostConfigOrShortcut::Full(HostConfig::Sprite(sprite_config)),
            container: None, // Sprites don't support containers
        }
    }

    /// Create a Fly runner config with container image.
    /// Fly machines ARE containers, so the image is required.
    pub fn fly(fly_config: FlyHostConfig, image: String) -> Self {
        RunnerConfig {
            host: HostConfigOrShortcut::Full(HostConfig::Fly(fly_config)),
            container: Some(ContainerConfig { image }),
        }
    }

    /// Check if this runner uses a Docker container.
    pub fn uses_container(&self) -> bool {
        self.container.is_some()
    }

    /// Get the resolved host type name.
    pub fn host_type(&self) -> &'static str {
        match self.host.resolve() {
            HostConfig::Local => "local",
            HostConfig::Client => "client",
            HostConfig::Ssh(_) => "ssh",
            HostConfig::Sprite(_) => "sprite",
            HostConfig::Fly(_) => "fly",
        }
    }

    /// Check if this runner is compatible with the given orchestrator mode.
    pub fn is_compatible_with(&self, mode: OrchestratorMode) -> bool {
        self.host.resolve().is_compatible_with(mode)
    }

    /// Validate this runner configuration for the given orchestrator mode.
    ///
    /// Returns an error if the runner is not compatible with the mode.
    pub fn validate_for_mode(&self, mode: OrchestratorMode) -> RunnerResult<()> {
        let host = self.host.resolve();
        if !host.is_compatible_with(mode) {
            if let Some(reason) = host.local_incompatibility_reason() {
                return Err(RunnerError::IncompatibleMode(reason.to_string()));
            }
        }
        Ok(())
    }
}
