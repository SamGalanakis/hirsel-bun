//! Runner trait and implementations for spawning workers on different platforms.
//!
//! This module provides a unified interface for spawning worker processes
//! across different execution environments using a Host + Container model:
//!
//! ## Hosts (where compute runs)
//! - `local` - on this machine (contextual: GUI/CLI machine in local mode, orchestrator in remote mode)
//! - `ssh` - remote machine via SSH
//! - `sprite` - Sprites.dev cloud VM
//! - `fly` - Fly.io ephemeral machines
//! - `client` - (remote mode only) SSH back to the GUI/CLI user's machine via Tailscale
//!
//! ## Containers (optional isolation)
//! - `none` (default) - run directly on host
//! - `docker` - run in Docker container (image URI only)
//!
//! ## Constraints
//! - Sprites cannot run Docker (Firecracker limitation)
//! - Fly requires container.image (machines ARE containers)
//! - `client` host only available in remote mode

pub mod fly;
pub mod local;
pub mod setup;
pub mod sprite;
pub mod ssh;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

use crate::core::credentials::ForwardedCredentials;

// Re-export runner implementations
pub use fly::FlyRunner;
pub use local::LocalRunner;
pub use sprite::SpriteRunner;
pub use ssh::SshRunner;

/// Errors that can occur during runner operations
#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("Failed to spawn worker: {0}")]
    SpawnFailed(String),

    #[error("Failed to stop worker: {0}")]
    StopFailed(String),

    #[error("Worker not found: {0}")]
    WorkerNotFound(String),

    #[error("Setup failed: {0}")]
    SetupFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SSH error: {0}")]
    Ssh(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("State error: {0}")]
    State(String),

    #[error("Run is paused")]
    RunPaused,

    #[error("Timeout: {0}")]
    Timeout(String),
}

pub type RunnerResult<T> = Result<T, RunnerError>;

/// Configuration for spawning a worker
#[derive(Debug, Clone)]
pub struct WorkerSpawnConfig {
    /// Run name
    pub run_name: String,
    /// Worker name
    pub worker_name: String,
    /// Working directory for the worker
    pub work_dir: PathBuf,
    /// Run directory (contains state.db, chats/, logs/)
    pub run_dir: PathBuf,
    /// Path to spec file
    pub spec_path: PathBuf,
    /// Agent command to run (e.g., ["hirsel", "__acp-bridge"])
    pub agent_command: Vec<String>,
    /// Whether this worker is the leader
    pub is_leader: bool,
    /// Name of the leader worker (if known)
    pub leader_name: Option<String>,
    /// List of teammate worker names
    pub teammates: Option<Vec<String>>,
    /// Session ID to resume (optional)
    pub resume_session_id: Option<String>,
    /// Environment variables to pass to the worker (legacy, prefer credentials)
    pub env_vars: Option<HashMap<String, String>>,
    /// Credentials to forward to the worker (OAuth tokens, API keys)
    pub credentials: Option<ForwardedCredentials>,
    /// URL for coordinator API (for remote workers)
    pub coordinator_url: Option<String>,
    /// Tailscale auth key for auto-joining worker hosts to tailnet
    pub tailscale_authkey: Option<String>,
}

impl WorkerSpawnConfig {
    /// Collect environment variables to pass to the worker.
    ///
    /// This merges:
    /// 1. Explicit env_vars
    /// 2. Credentials (API key as ANTHROPIC_API_KEY, OAuth as CLAUDE_CODE_OAUTH_TOKEN)
    ///
    /// Credentials take precedence over env_vars for overlapping keys.
    pub fn collect_env_vars(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();

        // Start with explicit env_vars
        if let Some(ref vars) = self.env_vars {
            env.extend(vars.clone());
        }

        // Add credentials (override env_vars)
        if let Some(ref creds) = self.credentials {
            if let Some(ref key) = creds.anthropic_api_key {
                env.insert("ANTHROPIC_API_KEY".to_string(), key.clone());
            }
            if let Some(ref token) = creds.claude_access_token {
                env.insert("CLAUDE_CODE_OAUTH_TOKEN".to_string(), token.clone());
            }
        }

        env
    }
}

/// Handle to a spawned worker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerHandle {
    /// Worker name
    pub worker_name: String,
    /// Runner-specific identifier (PID for local, sprite name for sprites, etc.)
    pub runner_id: String,
    /// Runner type that spawned this worker
    pub runner_type: String,
}

/// Result of spawning a worker
#[derive(Debug)]
pub struct SpawnResult {
    /// Worker handle for lifecycle management
    pub handle: WorkerHandle,
    /// Process ID (if applicable)
    pub pid: Option<u32>,
}

/// Trait for worker runners - implementations spawn and manage workers
/// on different platforms (local, SSH, Sprites).
#[async_trait]
pub trait Runner: Send + Sync {
    /// Spawn a worker on this runner.
    ///
    /// Returns a handle that can be used to manage the worker's lifecycle.
    async fn spawn(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult>;

    /// Stop a worker.
    ///
    /// Attempts graceful shutdown first, then force-kills if necessary.
    async fn stop(&self, handle: &WorkerHandle) -> RunnerResult<()>;

    /// Check if a worker is still alive.
    async fn is_alive(&self, handle: &WorkerHandle) -> bool;

    /// Get runner type name for display.
    fn runner_type(&self) -> &'static str;

    /// Setup the runner environment.
    ///
    /// This is called once before spawning any workers. Implementations
    /// can use this to install dependencies, create directories, etc.
    async fn setup(&self) -> RunnerResult<()> {
        Ok(()) // default no-op
    }

    /// Cleanup when done.
    ///
    /// Called when the run completes or is terminated.
    async fn cleanup(&self) -> RunnerResult<()> {
        Ok(()) // default no-op
    }

    /// Get logs from a worker.
    ///
    /// Returns the last N lines of the worker's log output.
    /// Note: File-based logging has been removed; use the worker events API instead.
    async fn get_logs(&self, _handle: &WorkerHandle, _lines: usize) -> RunnerResult<String> {
        // Worker events are now stored in the database
        // Use the orchestrator's get_worker_events method instead
        Ok(String::new())
    }
}

// =============================================================================
// Host + Container Model
// =============================================================================

/// Container configuration for running workers in Docker.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContainerConfig {
    /// Docker image URI (e.g., "rust:latest", "ghcr.io/org/dev-env")
    pub image: String,
}

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

/// Host configuration - defines where compute runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HostConfig {
    /// Local host - run on this machine
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

impl Default for HostConfig {
    fn default() -> Self {
        HostConfig::Local
    }
}

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

/// Runner configuration using Host + Container model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerConfig {
    /// Host configuration (where compute runs)
    #[serde(default)]
    pub host: HostConfigOrShortcut,
    /// Optional container configuration (Docker)
    #[serde(default)]
    pub container: Option<ContainerConfig>,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        RunnerConfig {
            host: HostConfigOrShortcut::default(),
            container: None,
        }
    }
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
}

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
// Runner Factory
// =============================================================================

/// Create a runner from configuration.
pub fn create_runner(config: &RunnerConfig) -> Box<dyn Runner> {
    let host = config.host.resolve();
    match host {
        HostConfig::Local | HostConfig::Client => {
            Box::new(LocalRunner::new(config.container.clone()))
        }
        HostConfig::Ssh(ssh_config) => {
            // Convert SshHostConfig to SshRunnerConfig for backwards compatibility
            let ssh_runner_config = SshRunnerConfig {
                host: ssh_config.address,
                ssh_key: ssh_config.ssh_key,
                ssh_port: ssh_config.port,
                work_base: ssh_config.work_base,
                location: ssh_config.location,
            };
            Box::new(SshRunner::new(ssh_runner_config, config.container.clone()))
        }
        HostConfig::Sprite(sprite_config) => {
            if config.container.is_some() {
                tracing::warn!("Sprites do not support containers - ignoring container config");
            }
            // Convert SpriteHostConfig to SpriteRunnerConfig for backwards compatibility
            let sprite_runner_config = SpriteRunnerConfig {
                api_token: sprite_config.api_token,
                base_checkpoint: sprite_config.checkpoint,
                auto_destroy: sprite_config.auto_destroy,
                idle_timeout_secs: sprite_config.idle_timeout_secs,
                api_url: sprite_config.api_url,
                use_file_push: sprite_config.use_file_push,
            };
            Box::new(SpriteRunner::new(sprite_runner_config))
        }
        HostConfig::Fly(fly_config) => {
            let image = config
                .container
                .as_ref()
                .map(|c| c.image.clone())
                .unwrap_or_else(|| {
                    tracing::warn!(
                        "Fly host should have container.image set, using debian:bookworm-slim"
                    );
                    "debian:bookworm-slim".to_string()
                });
            Box::new(FlyRunner::new(fly_config, image))
        }
    }
}

// =============================================================================
// Legacy Config Types (for backwards compatibility with existing code)
// =============================================================================

/// Configuration for SSH runner (legacy format, used internally).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshRunnerConfig {
    /// SSH host (e.g., "user@server.example.com")
    pub host: String,
    /// Path to SSH private key (optional)
    #[serde(default)]
    pub ssh_key: Option<String>,
    /// SSH port
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    /// Base directory for work on remote
    #[serde(default = "default_work_base")]
    pub work_base: String,
    /// Display name for location
    #[serde(default)]
    pub location: Option<String>,
}

impl Default for SshRunnerConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            ssh_key: None,
            ssh_port: 22,
            work_base: "/tmp/hirsel-remote".to_string(),
            location: None,
        }
    }
}

/// Configuration for Sprites runner (legacy format, used internally).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteRunnerConfig {
    /// Sprites API token (stored directly)
    #[serde(default)]
    pub api_token: Option<String>,
    /// Base checkpoint to clone from (pre-configured image)
    #[serde(default)]
    pub base_checkpoint: Option<String>,
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
    #[serde(default)]
    pub use_file_push: bool,
}

impl Default for SpriteRunnerConfig {
    fn default() -> Self {
        Self {
            api_token: None,
            base_checkpoint: None,
            auto_destroy: true,
            idle_timeout_secs: 30,
            api_url: "https://api.sprites.dev".to_string(),
            use_file_push: false,
        }
    }
}

// =============================================================================
// Utility Functions
// =============================================================================

/// Parse a remote specification like "user@host:count" or "user@host"
///
/// # Returns
/// Tuple of (host, count)
pub fn parse_remote_spec(spec: &str) -> (String, u32) {
    // Check for format: user@host:count
    if let Some(colon_idx) = spec.rfind(':') {
        let count_part = &spec[colon_idx + 1..];
        if let Ok(count) = count_part.parse::<u32>() {
            let host = spec[..colon_idx].to_string();
            return (host, count);
        }
    }

    // Format: user@host (count defaults to 1)
    (spec.to_string(), 1)
}

/// Parse multiple remote specifications
///
/// # Returns
/// List of (host, count) tuples
pub fn parse_remote_specs(specs: &[String]) -> Vec<(String, u32)> {
    specs.iter().map(|s| parse_remote_spec(s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_remote_spec_with_count() {
        let (host, count) = parse_remote_spec("user@server.com:3");
        assert_eq!(host, "user@server.com");
        assert_eq!(count, 3);
    }

    #[test]
    fn test_parse_remote_spec_without_count() {
        let (host, count) = parse_remote_spec("user@server.com");
        assert_eq!(host, "user@server.com");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_parse_remote_specs() {
        let specs = vec![
            "user@server1.com:2".to_string(),
            "user@server2.com".to_string(),
        ];
        let parsed = parse_remote_specs(&specs);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], ("user@server1.com".to_string(), 2));
        assert_eq!(parsed[1], ("user@server2.com".to_string(), 1));
    }

    #[test]
    fn test_runner_config_default() {
        let config = RunnerConfig::default();
        assert_eq!(config.host_type(), "local");
        assert!(config.container.is_none());
    }

    #[test]
    fn test_runner_config_local_docker() {
        let config = RunnerConfig::local_docker("rust:latest".to_string());
        assert_eq!(config.host_type(), "local");
        assert!(config.uses_container());
        assert_eq!(config.container.as_ref().unwrap().image, "rust:latest");
    }

    #[test]
    fn test_host_config_shortcut_resolution() {
        let shortcut = HostConfigOrShortcut::Shortcut("local".to_string());
        assert!(matches!(shortcut.resolve(), HostConfig::Local));

        let shortcut = HostConfigOrShortcut::Shortcut("client".to_string());
        assert!(matches!(shortcut.resolve(), HostConfig::Client));
    }

    #[test]
    fn test_ssh_config_default() {
        let config = SshHostConfig::default();
        assert_eq!(config.port, 22);
        assert_eq!(config.work_base, "/tmp/hirsel-remote");
    }

    #[test]
    fn test_sprite_config_default() {
        let config = SpriteHostConfig::default();
        assert!(config.api_token.is_none());
        assert!(config.auto_destroy);
        assert_eq!(config.idle_timeout_secs, 30);
    }

    #[test]
    fn test_runner_config_serialization() {
        let config = RunnerConfig::sprite(SpriteHostConfig {
            api_token: Some("my-secret-token".to_string()),
            checkpoint: Some("hirsel-v1".to_string()),
            auto_destroy: false,
            idle_timeout_secs: 60,
            api_url: "https://api.sprites.dev".to_string(),
            use_file_push: false,
        });

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("sprite"));
        assert!(json.contains("my-secret-token"));

        let parsed: RunnerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.host_type(), "sprite");
    }
}
