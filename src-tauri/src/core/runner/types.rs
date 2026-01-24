//! Core runner types and traits.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

use crate::core::credentials::ForwardedCredentials;

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

    #[error("Runner incompatible with orchestrator mode: {0}")]
    IncompatibleMode(String),
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
    /// Runner-specific identifier (PID for local, machine ID for Fly, etc.)
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
/// on different platforms (local, SSH, Fly).
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

    /// Does the work directory persist across worker restarts?
    ///
    /// Returns `false` for runners where files remain on disk between restarts:
    /// - Local (bare process): files on local disk
    /// - Docker (volume mount): host directory persists
    /// - SSH: files on remote disk
    ///
    /// Returns `true` for ephemeral runners where machines are destroyed:
    /// - Fly.io: machines are destroyed on stop
    ///
    /// The orchestrator uses this to decide whether to restore snapshots
    /// during resume operations.
    fn is_ephemeral(&self) -> bool {
        false // default: persistent (most runners)
    }
}

/// Orchestrator mode - determines which runners are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestratorMode {
    /// Local mode - uses daemon TCP listener for remote hosts.
    /// Compatible hosts: Local, SSH (via reverse tunnel to daemon TCP on localhost:19700)
    Local,
    /// Remote mode - HTTP API coordinator required.
    /// Compatible hosts: All (Local, SSH, Fly, Client)
    Remote,
}
