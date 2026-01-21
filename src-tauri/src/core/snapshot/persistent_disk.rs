//! Persistent disk snapshot strategy.
//!
//! This is a no-op strategy for hosts where files persist across restarts:
//! - Local host (files on local disk)
//! - SSH host (files on remote disk)
//! - Docker containers with volume mounts
//!
//! Since files remain in place, we just verify the work directory exists.

use async_trait::async_trait;
use chrono::Utc;
use std::path::Path;
use tracing::debug;

use super::{SnapshotError, SnapshotHandle, SnapshotResult, SnapshotStrategy};

/// Persistent disk strategy - no-op since files remain on disk.
#[derive(Debug, Clone)]
pub struct PersistentDiskStrategy;

impl PersistentDiskStrategy {
    /// Create a new persistent disk strategy.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PersistentDiskStrategy {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SnapshotStrategy for PersistentDiskStrategy {
    async fn snapshot(
        &self,
        run_name: &str,
        worker_name: &str,
        work_dir: &Path,
    ) -> SnapshotResult<SnapshotHandle> {
        // Verify work directory exists
        if !work_dir.exists() {
            return Err(SnapshotError::WorkDirNotFound(
                work_dir.to_string_lossy().to_string(),
            ));
        }

        debug!(
            "PersistentDisk snapshot for {}/{}: work_dir exists at {:?}",
            run_name, worker_name, work_dir
        );

        // Create a handle that references the work directory path
        // This is used for tracking purposes but no actual data transfer happens
        Ok(SnapshotHandle {
            strategy_type: self.strategy_type().to_string(),
            snapshot_id: work_dir.to_string_lossy().to_string(),
            created_at: Utc::now().to_rfc3339(),
            size_bytes: None,
        })
    }

    async fn restore(&self, handle: &SnapshotHandle, work_dir: &Path) -> SnapshotResult<()> {
        // For persistent disk, we just verify the directory still exists
        // The snapshot_id contains the original work_dir path

        if !work_dir.exists() {
            return Err(SnapshotError::WorkDirNotFound(
                work_dir.to_string_lossy().to_string(),
            ));
        }

        debug!(
            "PersistentDisk restore: work_dir exists at {:?} (snapshot_id: {})",
            work_dir, handle.snapshot_id
        );

        Ok(())
    }

    async fn delete(&self, handle: &SnapshotHandle) -> SnapshotResult<()> {
        // No-op for persistent disk - nothing to clean up
        debug!(
            "PersistentDisk delete: no-op for snapshot {}",
            handle.snapshot_id
        );
        Ok(())
    }

    fn strategy_type(&self) -> &'static str {
        "persistent_disk"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_persistent_disk_snapshot() {
        let temp = TempDir::new().unwrap();
        let work_dir = temp.path();

        let strategy = PersistentDiskStrategy::new();
        let handle = strategy
            .snapshot("test-run", "worker1", work_dir)
            .await
            .unwrap();

        assert_eq!(handle.strategy_type, "persistent_disk");
        assert_eq!(handle.snapshot_id, work_dir.to_string_lossy());
        assert!(handle.size_bytes.is_none());
    }

    #[tokio::test]
    async fn test_persistent_disk_restore() {
        let temp = TempDir::new().unwrap();
        let work_dir = temp.path();

        let strategy = PersistentDiskStrategy::new();
        let handle = strategy
            .snapshot("test-run", "worker1", work_dir)
            .await
            .unwrap();

        // Restore should succeed if directory exists
        strategy.restore(&handle, work_dir).await.unwrap();
    }

    #[tokio::test]
    async fn test_persistent_disk_missing_dir() {
        let strategy = PersistentDiskStrategy::new();

        // Snapshot should fail if directory doesn't exist
        let result = strategy
            .snapshot("test-run", "worker1", Path::new("/nonexistent/path"))
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SnapshotError::WorkDirNotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_persistent_disk_delete() {
        let temp = TempDir::new().unwrap();
        let work_dir = temp.path();

        let strategy = PersistentDiskStrategy::new();
        let handle = strategy
            .snapshot("test-run", "worker1", work_dir)
            .await
            .unwrap();

        // Delete should succeed (no-op)
        strategy.delete(&handle).await.unwrap();

        // Directory should still exist (not deleted)
        assert!(work_dir.exists());
    }
}
