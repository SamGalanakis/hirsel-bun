//! Types for service worker management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::core::config::Config;

/// Types of service workers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServiceWorkerType {
    /// Scribe - documentation agent
    Scribe,
    /// ConflictResolver - merge conflict resolution agent
    ConflictResolver,
}

impl ServiceWorkerType {
    /// Get the CLI type argument value
    pub fn as_str(&self) -> &'static str {
        match self {
            ServiceWorkerType::Scribe => "scribe",
            ServiceWorkerType::ConflictResolver => "conflict-resolver",
        }
    }

    /// Get default idle timeout in seconds
    pub fn default_idle_timeout(&self) -> u32 {
        match self {
            ServiceWorkerType::Scribe => 300,           // 5 minutes
            ServiceWorkerType::ConflictResolver => 600, // 10 minutes (conflicts can take longer)
        }
    }
}

impl std::fmt::Display for ServiceWorkerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Handle to a running service worker
#[derive(Debug, Clone)]
pub struct ServiceWorkerHandle {
    /// Type of service worker
    pub worker_type: ServiceWorkerType,
    /// HTTP endpoint for the worker (e.g., "http://localhost:12345")
    pub endpoint: String,
    /// Runner type that spawned this worker
    pub runner_type: String,
    /// Runner-specific identifier (PID, machine ID, etc.)
    pub runner_id: String,
    /// When the worker was spawned
    pub spawned_at: DateTime<Utc>,
}

impl ServiceWorkerHandle {
    /// Create a new service worker handle
    pub fn new(
        worker_type: ServiceWorkerType,
        endpoint: String,
        runner_type: String,
        runner_id: String,
    ) -> Self {
        Self {
            worker_type,
            endpoint,
            runner_type,
            runner_id,
            spawned_at: Utc::now(),
        }
    }

    /// Get the health check URL
    pub fn health_url(&self) -> String {
        format!("{}/health", self.endpoint)
    }

    /// Get the scribe batch URL
    pub fn scribe_batch_url(&self) -> String {
        format!("{}/scribe/batch", self.endpoint)
    }
}

/// Service worker errors
#[derive(Debug, Error)]
pub enum ServiceWorkerError {
    #[error("Failed to spawn service worker: {0}")]
    SpawnFailed(String),

    #[error("Service worker not available: {0}")]
    NotAvailable(String),

    #[error("Health check failed: {0}")]
    HealthCheckFailed(String),

    #[error("Request failed: {0}")]
    RequestFailed(String),

    #[error("Runner error: {0}")]
    Runner(#[from] crate::core::runner::RunnerError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("No runner configured for service worker")]
    NoRunnerConfigured,

    #[error("Configuration error: {0}")]
    Config(String),
}

pub type ServiceWorkerResult<T> = Result<T, ServiceWorkerError>;

// =============================================================================
// Service Worker Base
// =============================================================================

/// Base functionality shared by all service worker implementations.
///
/// Handles the common patterns:
/// - Remote worker lifecycle (spawn, health check, stop)
/// - Deciding whether to use local or remote execution
///
/// # Usage
///
/// ```ignore
/// pub struct MyService {
///     base: ServiceWorkerBase,
/// }
///
/// impl MyService {
///     async fn do_work(&self) -> Result<(), MyError> {
///         if self.base.should_use_remote() {
///             let endpoint = self.base.get_or_spawn_worker().await?;
///             // Use endpoint...
///         } else {
///             // Local execution...
///         }
///         Ok(())
///     }
/// }
/// ```
pub struct ServiceWorkerBase {
    config: Config,
    worker_type: ServiceWorkerType,
    remote_worker: RwLock<Option<ServiceWorkerHandle>>,
}

impl ServiceWorkerBase {
    /// Create a new service worker base.
    pub fn new(config: Config, worker_type: ServiceWorkerType) -> Self {
        Self {
            config,
            worker_type,
            remote_worker: RwLock::new(None),
        }
    }

    /// Get the worker type.
    pub fn worker_type(&self) -> ServiceWorkerType {
        self.worker_type
    }

    /// Get a reference to the config.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Check if we should use a remote service worker.
    pub fn should_use_remote(&self) -> bool {
        let runner = self.get_runner_name();
        match runner {
            None => false,                        // No config → local
            Some("local") => false,               // Explicit local
            Some(r) => self.config.has_runner(r), // Check if runner exists
        }
    }

    /// Get the configured runner name for this worker type.
    fn get_runner_name(&self) -> Option<&str> {
        match self.worker_type {
            ServiceWorkerType::Scribe => self.config.service_workers.scribe_runner(),
            ServiceWorkerType::ConflictResolver => {
                self.config.service_workers.conflict_resolver_runner()
            }
        }
    }

    /// Get the idle timeout for this worker type.
    fn get_idle_timeout(&self) -> u32 {
        match self.worker_type {
            ServiceWorkerType::Scribe => self.config.service_workers.scribe_idle_timeout(),
            ServiceWorkerType::ConflictResolver => {
                self.config.service_workers.conflict_resolver_idle_timeout()
            }
        }
    }

    /// Log name for this service.
    fn log_name(&self) -> &'static str {
        match self.worker_type {
            ServiceWorkerType::Scribe => "ScribeService",
            ServiceWorkerType::ConflictResolver => "ConflictResolverService",
        }
    }

    /// Get or spawn a remote worker, returning the endpoint URL.
    pub async fn get_or_spawn_worker(&self) -> ServiceWorkerResult<String> {
        // Check if we have a healthy existing worker
        {
            let guard = self.remote_worker.read().await;
            if let Some(handle) = guard.as_ref() {
                if self.health_check(handle).await {
                    return Ok(handle.endpoint.clone());
                }
            }
        }

        // Need to spawn a new worker
        let handle = self.spawn_worker().await?;
        let endpoint = handle.endpoint.clone();

        // Store the handle
        let mut guard = self.remote_worker.write().await;
        *guard = Some(handle);

        Ok(endpoint)
    }

    /// Spawn a service worker process.
    async fn spawn_worker(&self) -> ServiceWorkerResult<ServiceWorkerHandle> {
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, BufReader};
        use tokio::process::Command;

        let runner_name = self
            .get_runner_name()
            .ok_or(ServiceWorkerError::NoRunnerConfigured)?;

        info!(
            "[{}] Spawning service worker on runner '{}'",
            self.log_name(),
            runner_name
        );

        let exe = std::env::current_exe()
            .map_err(|e| ServiceWorkerError::SpawnFailed(format!("Failed to get exe: {}", e)))?;

        let idle_timeout = self.get_idle_timeout();
        let type_arg = self.worker_type.as_str();

        // Spawn the service worker process
        let mut child = Command::new(&exe)
            .arg("__service-worker")
            .arg("--type")
            .arg(type_arg)
            .arg("--idle-timeout")
            .arg(idle_timeout.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| ServiceWorkerError::SpawnFailed(format!("Failed to spawn: {}", e)))?;

        let pid = child.id().unwrap_or(0);

        // Read the port from stdout (first line)
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ServiceWorkerError::SpawnFailed("Failed to get stdout".into()))?;

        let mut reader = BufReader::new(stdout);
        let mut line = String::new();

        // Wait for the ready message with timeout
        let read_result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            reader.read_line(&mut line),
        )
        .await;

        match read_result {
            Ok(Ok(0)) => Err(ServiceWorkerError::SpawnFailed(
                "Worker exited before ready".into(),
            )),
            Ok(Ok(_)) => {
                // Parse the JSON ready message
                // Expected format: {"status":"ready","port":12345}
                if let Ok(ready) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(port) = ready.get("port").and_then(|p| p.as_u64()) {
                        let endpoint = format!("http://127.0.0.1:{}", port);
                        debug!(
                            "[{}] Worker ready on port {} (pid {})",
                            self.log_name(),
                            port,
                            pid
                        );
                        return Ok(ServiceWorkerHandle::new(
                            self.worker_type,
                            endpoint,
                            "local".to_string(),
                            pid.to_string(),
                        ));
                    }
                }
                Err(ServiceWorkerError::SpawnFailed(format!(
                    "Invalid ready message: {}",
                    line.trim()
                )))
            }
            Ok(Err(e)) => Err(ServiceWorkerError::SpawnFailed(format!(
                "Read error: {}",
                e
            ))),
            Err(_) => {
                // Kill the child on timeout
                let _ = child.kill().await;
                Err(ServiceWorkerError::SpawnFailed(
                    "Timeout waiting for ready".into(),
                ))
            }
        }
    }

    /// Check if a service worker is healthy.
    pub async fn health_check(&self, handle: &ServiceWorkerHandle) -> bool {
        let url = handle.health_url();
        let client = reqwest::Client::new();

        match client
            .get(&url)
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => true,
            Ok(_) => {
                debug!("[{}] Health check failed: bad status", self.log_name());
                false
            }
            Err(e) => {
                debug!("[{}] Health check failed: {}", self.log_name(), e);
                false
            }
        }
    }

    /// Stop the remote worker if running.
    pub async fn stop(&self) -> ServiceWorkerResult<()> {
        let handle = {
            let mut guard = self.remote_worker.write().await;
            guard.take()
        };

        if let Some(handle) = handle {
            if handle.runner_type == "local" {
                if let Ok(pid) = handle.runner_id.parse::<u32>() {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
                    }
                    info!("[{}] Stopped worker (pid {})", self.log_name(), pid);
                }
            } else {
                warn!(
                    "[{}] Cannot stop remote worker {} (type: {})",
                    self.log_name(),
                    handle.runner_id,
                    handle.runner_type
                );
            }
        }

        Ok(())
    }
}
