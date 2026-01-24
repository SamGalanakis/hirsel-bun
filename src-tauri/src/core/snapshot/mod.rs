//! Archive strategy system for worker pause/resume.
//!
//! This module provides a unified interface for archiving worker state
//! across pause/resume cycles. The strategy is determined by host type:
//!
//! - **NoOp**: For Local and SSH hosts where files persist on disk.
//! - **S3**: For Fly hosts where machines are destroyed.
//!
//! # Usage
//!
//! ```rust,ignore
//! use hirsel_lib::core::snapshot::{create_archive_strategy, ArchiveStrategy};
//!
//! // Create strategy based on runner config
//! let strategy = create_archive_strategy(&runner_config, &storage_config).await?;
//!
//! // Archive before stopping
//! let handle = strategy.archive("run/worker/workdir", &work_dir).await?;
//!
//! // Restore after starting
//! strategy.restore(&handle, &work_dir).await?;
//! ```

mod archive;
mod claude_session;
mod noop;
#[cfg(feature = "s3-storage")]
mod s3;

pub use archive::{ArchiveHandle, ArchiveResult, ArchiveStrategy};
pub use claude_session::{claude_session_dir, host_session_path};
pub use noop::NoOpArchiveStrategy;
#[cfg(feature = "s3-storage")]
pub use s3::S3ArchiveStrategy;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::config::StorageConfig;
use crate::core::runner::{HostConfig, RunnerConfig};

// =============================================================================
// Unified Worker State Handle
// =============================================================================

/// Unified handle for all worker state that needs to persist across pause/resume.
///
/// Contains optional work directory and agent session snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkerStateHandle {
    /// Snapshot of the work directory (for ephemeral runners).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_dir: Option<WorkDirSnapshot>,
    /// Snapshot of the agent session (e.g., ~/.claude).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<AgentSnapshot>,
}

impl WorkerStateHandle {
    /// Create a new empty state handle.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with just a work directory snapshot.
    pub fn with_work_dir(snapshot: WorkDirSnapshot) -> Self {
        Self {
            work_dir: Some(snapshot),
            agent_session: None,
        }
    }

    /// Create with just an agent session snapshot.
    pub fn with_agent_session(snapshot: AgentSnapshot) -> Self {
        Self {
            work_dir: None,
            agent_session: Some(snapshot),
        }
    }

    /// Check if this handle has any state to restore.
    pub fn has_state(&self) -> bool {
        self.work_dir.is_some() || self.agent_session.is_some()
    }
}

/// Snapshot of a worker's work directory.
///
/// Used to persist and restore the work directory for ephemeral runners
/// (Fly with S3 strategy) across pause/resume cycles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkDirSnapshot {
    /// Type of strategy that created this snapshot (e.g., "s3", "persistent_disk").
    pub strategy_type: String,
    /// Storage identifier (S3 key, local path, etc.).
    pub storage_id: String,
    /// Size of the snapshot in bytes (if known).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

impl WorkDirSnapshot {
    /// Create a new work directory snapshot.
    pub fn new(strategy_type: impl Into<String>, storage_id: impl Into<String>) -> Self {
        Self {
            strategy_type: strategy_type.into(),
            storage_id: storage_id.into(),
            size_bytes: None,
        }
    }

    /// Create with size information.
    pub fn with_size(
        strategy_type: impl Into<String>,
        storage_id: impl Into<String>,
        size_bytes: u64,
    ) -> Self {
        Self {
            strategy_type: strategy_type.into(),
            storage_id: storage_id.into(),
            size_bytes: Some(size_bytes),
        }
    }
}

/// Snapshot of an agent's session state.
///
/// Used to persist and restore agent session data (e.g., Claude's `.claude` directory)
/// across pause/resume cycles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    /// Type of agent (e.g., "claude").
    pub agent_type: String,
    /// Session ID for resuming the agent session.
    pub session_id: String,
    /// Storage identifier (S3 key, local path, etc.).
    pub storage_id: String,
}

impl AgentSnapshot {
    /// Create a new agent snapshot.
    pub fn new(
        agent_type: impl Into<String>,
        session_id: impl Into<String>,
        storage_id: impl Into<String>,
    ) -> Self {
        Self {
            agent_type: agent_type.into(),
            session_id: session_id.into(),
            storage_id: storage_id.into(),
        }
    }
}

/// Snapshot errors
#[derive(Debug, Error)]
pub enum SnapshotError {
    /// I/O error during snapshot operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Storage backend error
    #[error("Storage error: {0}")]
    Storage(String),

    /// S3 error
    #[error("S3 error: {0}")]
    S3(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Snapshot not found
    #[error("Snapshot not found: {0}")]
    NotFound(String),

    /// Work directory not found
    #[error("Work directory not found: {0}")]
    WorkDirNotFound(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Result type for snapshot operations
pub type SnapshotResult<T> = Result<T, SnapshotError>;

// =============================================================================
// Unified Archive Strategy Factory
// =============================================================================

/// Create an archive strategy based on runner and storage configuration.
///
/// This is the preferred factory for new code. It returns a unified `ArchiveStrategy`
/// that can be used for both work directories and agent sessions.
///
/// # Strategy Selection
///
/// | Host Type | Strategy |
/// |-----------|----------|
/// | Local | NoOpArchiveStrategy (files persist on disk) |
/// | SSH | NoOpArchiveStrategy (files persist on remote disk) |
/// | Fly | S3ArchiveStrategy (machines are destroyed, need S3 storage) |
/// | Client | NoOpArchiveStrategy (no archiving needed) |
pub async fn create_archive_strategy(
    runner_config: &RunnerConfig,
    storage_config: &StorageConfig,
) -> ArchiveResult<Box<dyn ArchiveStrategy>> {
    match runner_config.host.resolve() {
        HostConfig::Local | HostConfig::Ssh(_) | HostConfig::Client => {
            Ok(Box::new(NoOpArchiveStrategy::new()))
        }
        HostConfig::Fly(_) => {
            #[cfg(feature = "s3-storage")]
            {
                let s3_config = storage_config.get_storage(None).ok_or_else(|| {
                    SnapshotError::Config(
                        "Fly runner requires S3 storage configuration for pause/resume. \
                         Add a storage in Settings > Storage, or configure [storage.storages] in config.toml"
                            .into(),
                    )
                })?;
                let strategy = S3ArchiveStrategy::new(s3_config, None).await?;
                Ok(Box::new(strategy))
            }
            #[cfg(not(feature = "s3-storage"))]
            {
                let _ = storage_config;
                // Fall back to no-op - sessions won't persist across Fly machine restarts
                tracing::warn!(
                    "S3 storage feature not enabled - Fly sessions will not persist across restarts"
                );
                Ok(Box::new(NoOpArchiveStrategy::new()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_work_dir_snapshot_serialization() {
        let snapshot =
            WorkDirSnapshot::with_size("s3", "archives/my-run/worker1/workdir.tar.gz", 1024 * 1024);

        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: WorkDirSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(snapshot.strategy_type, restored.strategy_type);
        assert_eq!(snapshot.storage_id, restored.storage_id);
        assert_eq!(snapshot.size_bytes, restored.size_bytes);
    }

    #[test]
    fn test_worker_state_handle_serialization() {
        let handle = WorkerStateHandle::with_work_dir(WorkDirSnapshot::new("noop", "/path/to/dir"));

        let json = serde_json::to_string(&handle).unwrap();
        let restored: WorkerStateHandle = serde_json::from_str(&json).unwrap();

        assert!(restored.work_dir.is_some());
        assert!(restored.agent_session.is_none());
    }

    #[test]
    fn test_agent_snapshot_serialization() {
        let snapshot = AgentSnapshot::new("claude", "session-123", "s3://bucket/key");

        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: AgentSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(snapshot.agent_type, restored.agent_type);
        assert_eq!(snapshot.session_id, restored.session_id);
        assert_eq!(snapshot.storage_id, restored.storage_id);
    }
}
