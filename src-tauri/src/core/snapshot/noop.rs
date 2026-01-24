//! No-op archive strategy for persistent storage environments.
//!
//! This strategy is used when files persist on disk and don't need to be
//! explicitly archived/restored.
//!
//! # Use Cases
//!
//! - **Local host**: Files persist in the run directory
//! - **SSH host**: Files persist on the remote disk
//! - **Sprite VMs**: VM checkpoint captures all state (archive is handled by VM)
//! - **Docker with mounts**: Session directories are mounted from host

use async_trait::async_trait;
use std::path::Path;
use tracing::debug;

use super::archive::{ArchiveHandle, ArchiveResult, ArchiveStrategy};
use super::SnapshotError;

/// No-op archive strategy for environments where files persist.
///
/// This strategy performs minimal validation but doesn't actually copy
/// any data during archive/restore operations. It's used for:
///
/// - Local and SSH hosts where files remain on disk
/// - Sprite VMs where the VM checkpoint handles state preservation
/// - Docker containers where directories are bind-mounted from the host
#[derive(Debug, Clone, Default)]
pub struct NoOpArchiveStrategy;

impl NoOpArchiveStrategy {
    /// Create a new no-op archive strategy.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ArchiveStrategy for NoOpArchiveStrategy {
    async fn archive(&self, key: &str, source_dir: &Path) -> ArchiveResult<ArchiveHandle> {
        // Verify source directory exists (for debugging purposes)
        if !source_dir.exists() {
            debug!(
                "NoOp archive: source directory does not exist: {:?} (key: {})",
                source_dir, key
            );
            // Don't error - the directory might be created later or might not exist yet
        } else {
            debug!(
                "NoOp archive for '{}': directory exists at {:?}",
                key, source_dir
            );
        }

        // Return a handle that references the original path
        // This is used for tracking purposes but no actual data transfer happens
        Ok(ArchiveHandle::new(
            self.strategy_type(),
            source_dir.to_string_lossy().to_string(),
        ))
    }

    async fn restore(&self, handle: &ArchiveHandle, target_dir: &Path) -> ArchiveResult<()> {
        // For no-op strategy, we just verify the directory exists
        // The storage_id contains the original path (though we use target_dir)

        if !target_dir.exists() {
            // Create the directory if it doesn't exist
            // This is common for fresh restores where work_dir needs to be created
            std::fs::create_dir_all(target_dir).map_err(SnapshotError::Io)?;
            debug!(
                "NoOp restore: created directory {:?} (handle: {})",
                target_dir, handle.storage_id
            );
        } else {
            debug!(
                "NoOp restore: directory exists at {:?} (handle: {})",
                target_dir, handle.storage_id
            );
        }

        Ok(())
    }

    async fn delete(&self, handle: &ArchiveHandle) -> ArchiveResult<()> {
        // No-op - nothing to delete since we didn't copy anything
        debug!("NoOp delete: no-op for handle {}", handle.storage_id);
        Ok(())
    }

    fn strategy_type(&self) -> &'static str {
        "noop"
    }

    fn is_noop(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_noop_archive_existing_dir() {
        let temp = TempDir::new().unwrap();
        let strategy = NoOpArchiveStrategy::new();

        let handle = strategy
            .archive("test-run/worker1/workdir", temp.path())
            .await
            .unwrap();

        assert_eq!(handle.strategy_type, "noop");
        assert_eq!(handle.storage_id, temp.path().to_string_lossy());
        assert!(handle.size_bytes.is_none());
    }

    #[tokio::test]
    async fn test_noop_archive_nonexistent_dir() {
        let strategy = NoOpArchiveStrategy::new();

        // Should succeed even for non-existent directory
        let handle = strategy
            .archive("test-run/worker1/workdir", Path::new("/nonexistent/path"))
            .await
            .unwrap();

        assert_eq!(handle.strategy_type, "noop");
    }

    #[tokio::test]
    async fn test_noop_restore_existing_dir() {
        let temp = TempDir::new().unwrap();
        let strategy = NoOpArchiveStrategy::new();

        let handle = strategy
            .archive("test-run/worker1/workdir", temp.path())
            .await
            .unwrap();

        // Restore should succeed
        strategy.restore(&handle, temp.path()).await.unwrap();
    }

    #[tokio::test]
    async fn test_noop_restore_creates_dir() {
        let temp = TempDir::new().unwrap();
        let new_dir = temp.path().join("new_dir");

        let strategy = NoOpArchiveStrategy::new();
        let handle = ArchiveHandle::new("noop", new_dir.to_string_lossy().to_string());

        // Restore should create the directory
        strategy.restore(&handle, &new_dir).await.unwrap();
        assert!(new_dir.exists());
    }

    #[tokio::test]
    async fn test_noop_delete() {
        let temp = TempDir::new().unwrap();
        let strategy = NoOpArchiveStrategy::new();

        let handle = strategy
            .archive("test-run/worker1/workdir", temp.path())
            .await
            .unwrap();

        // Delete should succeed (no-op)
        strategy.delete(&handle).await.unwrap();

        // Directory should still exist (not deleted)
        assert!(temp.path().exists());
    }

    #[test]
    fn test_is_noop() {
        let strategy = NoOpArchiveStrategy::new();
        assert!(strategy.is_noop());
    }
}
