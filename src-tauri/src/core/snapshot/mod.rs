//! Snapshot strategy system for worker pause/resume.
//!
//! This module provides a unified interface for snapshotting worker state
//! across pause/resume cycles. The strategy is determined by host type:
//!
//! - **PersistentDisk**: For Local and SSH hosts where files persist on disk.
//! - **S3**: For Sprite and Fly hosts where machines are destroyed.
//!
//! # Usage
//!
//! ```rust,ignore
//! use hirsel_lib::core::snapshot::{create_snapshot_strategy, SnapshotStrategy};
//!
//! // Create strategy based on runner config
//! let strategy = create_snapshot_strategy(&runner_config, &storage_config).await?;
//!
//! // Snapshot before stopping
//! let handle = strategy.snapshot(&run_name, &worker_name, &work_dir).await?;
//!
//! // Restore after starting
//! strategy.restore(&handle, &work_dir).await?;
//! ```

mod persistent_disk;
#[cfg(feature = "s3-storage")]
mod s3;
mod sprite_checkpoint;

pub use persistent_disk::PersistentDiskStrategy;
#[cfg(feature = "s3-storage")]
pub use s3::S3SnapshotStrategy;
pub use sprite_checkpoint::SpriteCheckpointStrategy;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

use crate::core::config::StorageConfig;
use crate::core::runner::{HostConfig, RunnerConfig};

/// Snapshot errors
#[derive(Debug, Error)]
pub enum SnapshotError {
    /// I/O error during snapshot operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Storage backend error
    #[error("Storage error: {0}")]
    Storage(String),

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

/// Handle to a snapshot, stored in the database for later restoration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotHandle {
    /// Type of strategy that created this snapshot
    pub strategy_type: String,
    /// Unique identifier for the snapshot (S3 key, local path, etc.)
    pub snapshot_id: String,
    /// ISO 8601 timestamp when the snapshot was created
    pub created_at: String,
    /// Size of the snapshot in bytes (if known)
    pub size_bytes: Option<u64>,
}

/// Configuration for snapshot strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[derive(Default)]
pub enum SnapshotStrategyConfig {
    /// Persistent disk strategy - files remain on disk (no-op)
    #[default]
    PersistentDisk,
    /// S3 strategy - tar/gzip and upload to S3
    S3 {
        /// Optional prefix for S3 keys (default: "snapshots")
        #[serde(default)]
        prefix: Option<String>,
        /// Optional named storage config to use (default: use default_storage or legacy s3)
        #[serde(default)]
        storage: Option<String>,
    },
    /// Sprite checkpoint strategy - uses native Sprites.dev checkpoint API
    SpriteCheckpoint {
        /// Optional comment prefix for checkpoints (default: "hirsel-snapshot")
        #[serde(default)]
        comment_prefix: Option<String>,
    },
}

/// Trait for snapshot strategies.
///
/// Implementations handle snapshotting and restoring worker state
/// for different deployment scenarios.
#[async_trait]
pub trait SnapshotStrategy: Send + Sync {
    /// Create a snapshot of the worker's work directory.
    ///
    /// Called before stopping a worker during pause operations.
    async fn snapshot(
        &self,
        run_name: &str,
        worker_name: &str,
        work_dir: &Path,
    ) -> SnapshotResult<SnapshotHandle>;

    /// Restore a snapshot to the worker's work directory.
    ///
    /// Called before starting a worker during resume operations.
    async fn restore(&self, handle: &SnapshotHandle, work_dir: &Path) -> SnapshotResult<()>;

    /// Delete a snapshot.
    ///
    /// Called during cleanup (run deletion, successful restore, etc.)
    async fn delete(&self, handle: &SnapshotHandle) -> SnapshotResult<()>;

    /// Get the strategy type name.
    fn strategy_type(&self) -> &'static str;
}

/// Infer snapshot strategy from runner config and host type.
///
/// - Local and SSH hosts use PersistentDisk (files remain on disk)
/// - Sprite hosts use SpriteCheckpoint (native Sprites API)
/// - Fly hosts use S3 (machines are destroyed)
pub fn infer_snapshot_strategy(runner_config: &RunnerConfig) -> SnapshotStrategyConfig {
    match runner_config.snapshot {
        Some(ref config) => config.clone(),
        None => {
            // Infer from host type
            match runner_config.host.resolve() {
                HostConfig::Local | HostConfig::Ssh(_) => SnapshotStrategyConfig::PersistentDisk,
                HostConfig::Sprite(_) => SnapshotStrategyConfig::SpriteCheckpoint {
                    comment_prefix: None,
                },
                HostConfig::Fly(_) | HostConfig::Client => SnapshotStrategyConfig::S3 {
                    prefix: None,
                    storage: None,
                },
            }
        }
    }
}

/// Create a snapshot strategy based on configuration.
pub async fn create_snapshot_strategy(
    runner_config: &RunnerConfig,
    storage_config: &StorageConfig,
) -> SnapshotResult<Box<dyn SnapshotStrategy>> {
    let strategy_config = infer_snapshot_strategy(runner_config);

    match strategy_config {
        SnapshotStrategyConfig::PersistentDisk => Ok(Box::new(PersistentDiskStrategy::new())),
        SnapshotStrategyConfig::S3 { prefix, storage } => {
            #[cfg(feature = "s3-storage")]
            {
                let s3_config = storage_config
                    .get_storage(storage.as_deref())
                    .ok_or_else(|| {
                        SnapshotError::Config(
                            "S3 snapshot strategy requires storage configuration. \
                             Add a storage in Settings > Storage, or configure [storage.s3] in config.toml"
                                .into(),
                        )
                    })?;
                let strategy = S3SnapshotStrategy::new(s3_config, prefix).await?;
                Ok(Box::new(strategy))
            }
            #[cfg(not(feature = "s3-storage"))]
            {
                let _ = prefix;
                let _ = storage;
                let _ = storage_config;
                Err(SnapshotError::Config(
                    "S3 snapshot strategy requires --features s3-storage".into(),
                ))
            }
        }
        SnapshotStrategyConfig::SpriteCheckpoint { comment_prefix } => {
            // Get sprite config from runner
            let sprite_config = match runner_config.host.resolve() {
                HostConfig::Sprite(config) => config,
                _ => {
                    return Err(SnapshotError::Config(
                        "SpriteCheckpoint strategy requires a Sprite host".into(),
                    ))
                }
            };
            let strategy = SpriteCheckpointStrategy::new(&sprite_config, comment_prefix)?;
            Ok(Box::new(strategy))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_handle_serialization() {
        let handle = SnapshotHandle {
            strategy_type: "s3".to_string(),
            snapshot_id: "snapshots/my-run/worker1/2024-01-15T10:30:00Z.tar.gz".to_string(),
            created_at: "2024-01-15T10:30:00Z".to_string(),
            size_bytes: Some(1024 * 1024),
        };

        let json = serde_json::to_string(&handle).unwrap();
        let restored: SnapshotHandle = serde_json::from_str(&json).unwrap();

        assert_eq!(handle.strategy_type, restored.strategy_type);
        assert_eq!(handle.snapshot_id, restored.snapshot_id);
        assert_eq!(handle.size_bytes, restored.size_bytes);
    }

    #[test]
    fn test_infer_strategy_local() {
        let config = RunnerConfig::local();
        let strategy = infer_snapshot_strategy(&config);
        assert!(matches!(strategy, SnapshotStrategyConfig::PersistentDisk));
    }

    #[test]
    fn test_infer_strategy_fly() {
        use crate::core::runner::FlyHostConfig;

        let config = RunnerConfig::fly(
            FlyHostConfig {
                app: "test-app".to_string(),
                ..Default::default()
            },
            "debian:bookworm".to_string(),
        );
        let strategy = infer_snapshot_strategy(&config);
        assert!(matches!(strategy, SnapshotStrategyConfig::S3 { .. }));
    }

    #[test]
    fn test_infer_strategy_sprite() {
        use crate::core::runner::SpriteHostConfig;

        let config = RunnerConfig::sprite(SpriteHostConfig::default());
        let strategy = infer_snapshot_strategy(&config);
        assert!(matches!(
            strategy,
            SnapshotStrategyConfig::SpriteCheckpoint { .. }
        ));
    }
}
