//! Types for service worker management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Types of service workers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServiceWorkerType {
    /// Scribe - documentation agent
    Scribe,
}

impl ServiceWorkerType {
    /// Get the CLI type argument value
    pub fn as_str(&self) -> &'static str {
        match self {
            ServiceWorkerType::Scribe => "scribe",
        }
    }

    /// Get default idle timeout in seconds
    pub fn default_idle_timeout(&self) -> u32 {
        match self {
            ServiceWorkerType::Scribe => 300, // 5 minutes
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
