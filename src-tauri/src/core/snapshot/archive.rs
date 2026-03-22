//! Unified archive strategy for directory archival and restoration.
//!
//! This module provides the `ArchiveStrategy` trait for archiving both work directories
//! and agent sessions (e.g., `.codex`) to storage.
//!
//! # Design
//!
//! Work directories and agent sessions need the same core operations:
//! archive a directory to storage, restore from storage, and delete.
//! The trait uses a generic `key` parameter to identify archives, allowing
//! callers to use naming conventions like:
//!
//! - Work directory: `"{run_name}/{worker_name}/workdir"`
//! - Agent session: `"{run_name}/{worker_name}/session"`
//!
//! # Strategies
//!
//! | Strategy | Use Case | Implementation |
//! |----------|----------|----------------|
//! | `NoOpArchiveStrategy` | Persistent local hosts | Files persist on disk |
//! | `S3ArchiveStrategy` | Ephemeral hosts | Tar + upload to S3 |

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::SnapshotError;

/// Result type for archive operations.
pub type ArchiveResult<T> = Result<T, SnapshotError>;

/// Handle to an archived directory, stored in the database for later restoration.
///
/// A generic archive handle that can represent any archived directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveHandle {
    /// Type of strategy that created this archive (e.g., "noop", "s3").
    pub strategy_type: String,
    /// Storage identifier (S3 key, local path, etc.).
    pub storage_id: String,
    /// Size of the archive in bytes (if known).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

impl ArchiveHandle {
    /// Create a new archive handle.
    pub fn new(strategy_type: impl Into<String>, storage_id: impl Into<String>) -> Self {
        Self {
            strategy_type: strategy_type.into(),
            storage_id: storage_id.into(),
            size_bytes: None,
        }
    }

    /// Create an archive handle with size information.
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

/// Trait for directory archival strategies.
///
/// Implementations handle archiving and restoring directories for different
/// deployment scenarios. The strategy is determined by the runner configuration:
///
/// - **NoOp**: For persistent local hosts where files remain on disk
/// - **S3**: For ephemeral hosts where directories must be uploaded
#[async_trait]
pub trait ArchiveStrategy: Send + Sync {
    /// Archive a directory to storage.
    ///
    /// # Arguments
    /// * `key` - Unique key for this archive (e.g., "my-run/worker1/workdir")
    /// * `source_dir` - Path to the directory to archive
    ///
    /// # Returns
    /// A handle that can be used to restore or delete the archive.
    async fn archive(&self, key: &str, source_dir: &Path) -> ArchiveResult<ArchiveHandle>;

    /// Restore an archive to a directory.
    ///
    /// # Arguments
    /// * `handle` - Handle from a previous `archive()` call
    /// * `target_dir` - Path to restore the archive to
    async fn restore(&self, handle: &ArchiveHandle, target_dir: &Path) -> ArchiveResult<()>;

    /// Delete an archive from storage.
    ///
    /// # Arguments
    /// * `handle` - Handle from a previous `archive()` call
    async fn delete(&self, handle: &ArchiveHandle) -> ArchiveResult<()>;

    /// Get the strategy type name.
    fn strategy_type(&self) -> &'static str;

    /// Check if this strategy is a no-op (files persist on disk).
    ///
    /// This is useful for callers that want to skip archive operations
    /// for local runners where files already persist.
    fn is_noop(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_archive_handle_serialization() {
        let handle =
            ArchiveHandle::with_size("s3", "snapshots/my-run/worker1/workdir.tar.gz", 1024);

        let json = serde_json::to_string(&handle).unwrap();
        let restored: ArchiveHandle = serde_json::from_str(&json).unwrap();

        assert_eq!(handle.strategy_type, restored.strategy_type);
        assert_eq!(handle.storage_id, restored.storage_id);
        assert_eq!(handle.size_bytes, restored.size_bytes);
    }

    #[test]
    fn test_archive_handle_without_size() {
        let handle = ArchiveHandle::new("noop", "/path/to/dir");

        let json = serde_json::to_string(&handle).unwrap();
        assert!(!json.contains("size_bytes")); // Should be omitted

        let restored: ArchiveHandle = serde_json::from_str(&json).unwrap();
        assert!(restored.size_bytes.is_none());
    }
}
