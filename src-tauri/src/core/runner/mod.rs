//! Runner trait and implementations for spawning workers on different platforms.
//!
//! This module provides a unified interface for spawning worker processes
//! across different execution environments:
//! - Local: spawns processes on the local machine
//! - SSH: spawns processes on remote machines via SSH
//! - Sprite: spawns processes on Sprites.dev cloud VMs

pub mod local;
pub mod sprite;
pub mod ssh;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

// Re-export runner implementations
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
    /// Agent command to run (e.g., ["claude-code-acp"])
    pub agent_command: Vec<String>,
    /// Whether this worker is the leader
    pub is_leader: bool,
    /// Name of the leader worker (if known)
    pub leader_name: Option<String>,
    /// List of teammate worker names
    pub teammates: Option<Vec<String>>,
    /// Session ID to resume (optional)
    pub resume_session_id: Option<String>,
    /// Environment variables to pass to the worker
    pub env_vars: Option<HashMap<String, String>>,
    /// URL for coordinator API (for remote workers)
    pub coordinator_url: Option<String>,
    /// Git URL for cloning project (for remote workers)
    pub project_url: Option<String>,
    /// Tailscale auth key for auto-joining worker hosts to tailnet
    pub tailscale_authkey: Option<String>,
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

/// Create a runner from configuration
pub fn create_runner(config: &RunnerConfig) -> Box<dyn Runner> {
    match config {
        RunnerConfig::Local => Box::new(LocalRunner::new()),
        RunnerConfig::Ssh(ssh_config) => Box::new(SshRunner::new(ssh_config.clone())),
        RunnerConfig::Sprite(sprite_config) => Box::new(SpriteRunner::new(sprite_config.clone())),
    }
}

/// Runner configuration - defines how to spawn workers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RunnerConfig {
    /// Local runner - spawns processes on the local machine
    Local,
    /// SSH runner - spawns processes on remote machines via SSH
    Ssh(SshRunnerConfig),
    /// Sprite runner - spawns processes on Sprites.dev cloud VMs
    Sprite(SpriteRunnerConfig),
}

impl Default for RunnerConfig {
    fn default() -> Self {
        RunnerConfig::Local
    }
}

/// Configuration for SSH runner
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

fn default_ssh_port() -> u16 {
    22
}

fn default_work_base() -> String {
    "/tmp/hirsel-remote".to_string()
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

/// Configuration for Sprites runner
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

impl Default for SpriteRunnerConfig {
    fn default() -> Self {
        Self {
            api_token: None,
            base_checkpoint: None,
            auto_destroy: true,
            idle_timeout_secs: 30,
            api_url: "https://api.sprites.dev".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runner_config_default() {
        let config = RunnerConfig::default();
        assert!(matches!(config, RunnerConfig::Local));
    }

    #[test]
    fn test_ssh_config_default() {
        let config = SshRunnerConfig::default();
        assert_eq!(config.ssh_port, 22);
        assert_eq!(config.work_base, "/tmp/hirsel-remote");
    }

    #[test]
    fn test_sprite_config_default() {
        let config = SpriteRunnerConfig::default();
        assert!(config.api_token.is_none());
        assert!(config.auto_destroy);
        assert_eq!(config.idle_timeout_secs, 30);
    }

    #[test]
    fn test_runner_config_serialization() {
        let config = RunnerConfig::Sprite(SpriteRunnerConfig {
            api_token: Some("my-secret-token".to_string()),
            base_checkpoint: Some("hirsel-v1".to_string()),
            auto_destroy: false,
            idle_timeout_secs: 60,
            api_url: "https://api.sprites.dev".to_string(),
        });

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("sprite"));
        assert!(json.contains("my-secret-token"));

        let parsed: RunnerConfig = serde_json::from_str(&json).unwrap();
        if let RunnerConfig::Sprite(s) = parsed {
            assert_eq!(s.api_token, Some("my-secret-token".to_string()));
            assert!(!s.auto_destroy);
        } else {
            panic!("Expected Sprite config");
        }
    }
}
