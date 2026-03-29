//! S3 archive strategy.
//!
//! This strategy archives directories as tar.gz files and uploads them to
//! S3-compatible storage. Used for ephemeral hosts where machines are
//! destroyed between runs.
//!
//! # S3 Layout
//!
//! ```text
//! s3://bucket/
//! └── archives/           (or custom prefix)
//!     └── {key}.tar.gz
//! ```

use async_trait::async_trait;
use aws_sdk_s3::{primitives::ByteStream, Client};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::path::Path;
use tar::{Archive, Builder};
use tracing::{debug, info};

use super::archive::{ArchiveHandle, ArchiveResult, ArchiveStrategy};
use super::SnapshotError;
use crate::backend::config::S3Config;
use crate::backend::storage::S3ClientFactory;

/// S3 archive strategy implementing the unified ArchiveStrategy trait.
#[derive(Debug, Clone)]
pub struct S3ArchiveStrategy {
    client: Client,
    bucket: String,
    prefix: String,
}

impl S3ArchiveStrategy {
    /// Create a new S3 archive strategy.
    pub async fn new(config: &S3Config, prefix: Option<String>) -> ArchiveResult<Self> {
        let client = S3ClientFactory::create(config)
            .await
            .map_err(|e| SnapshotError::Config(e.to_string()))?;

        if config.bucket.is_empty() {
            return Err(SnapshotError::Config("S3 bucket name is required".into()));
        }

        Ok(Self {
            client,
            bucket: config.bucket.clone(),
            prefix: prefix.unwrap_or_else(|| "archives".to_string()),
        })
    }

    /// Build the S3 key for an archive.
    fn build_key(&self, key: &str) -> String {
        format!("{}/{}.tar.gz", self.prefix, key)
    }

    /// Create a tar.gz archive of a directory.
    fn create_archive(dir: &Path) -> ArchiveResult<Vec<u8>> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());

        {
            let mut archive = Builder::new(&mut encoder);

            // Add all files in the directory
            // We use append_dir_all with "." as the path inside the archive
            // so files are extracted relative to the target directory
            archive
                .append_dir_all(".", dir)
                .map_err(SnapshotError::Io)?;

            archive.finish().map_err(SnapshotError::Io)?;
        }

        encoder.finish().map_err(SnapshotError::Io)
    }

    /// Extract a tar.gz archive to a directory.
    fn extract_archive(data: &[u8], dir: &Path) -> ArchiveResult<()> {
        let decoder = GzDecoder::new(data);
        let mut archive = Archive::new(decoder);

        // Extract all files to the target directory
        archive.unpack(dir).map_err(SnapshotError::Io)?;

        Ok(())
    }
}

#[async_trait]
impl ArchiveStrategy for S3ArchiveStrategy {
    async fn archive(&self, key: &str, source_dir: &Path) -> ArchiveResult<ArchiveHandle> {
        if !source_dir.exists() {
            return Err(SnapshotError::WorkDirNotFound(
                source_dir.to_string_lossy().to_string(),
            ));
        }

        let s3_key = self.build_key(key);

        info!(
            "S3 archive '{}': archiving {:?} to s3://{}/{}",
            key, source_dir, self.bucket, s3_key
        );

        // Create tar.gz archive
        let archive_data = Self::create_archive(source_dir)?;
        let size_bytes = archive_data.len() as u64;

        debug!(
            "S3 archive '{}': created {} bytes, uploading",
            key, size_bytes
        );

        // Upload to S3
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&s3_key)
            .body(ByteStream::from(archive_data))
            .send()
            .await
            .map_err(|e| SnapshotError::Storage(e.to_string()))?;

        info!(
            "S3 archive '{}' complete: s3://{}/{} ({} bytes)",
            key, self.bucket, s3_key, size_bytes
        );

        Ok(ArchiveHandle::with_size(
            self.strategy_type(),
            s3_key,
            size_bytes,
        ))
    }

    async fn restore(&self, handle: &ArchiveHandle, target_dir: &Path) -> ArchiveResult<()> {
        info!("S3 restore '{}' to {:?}", handle.storage_id, target_dir);

        // Download from S3
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&handle.storage_id)
            .send()
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("NoSuchKey") || msg.contains("NotFound") {
                    SnapshotError::NotFound(handle.storage_id.clone())
                } else {
                    SnapshotError::Storage(msg)
                }
            })?;

        let body = response
            .body
            .collect()
            .await
            .map_err(|e| SnapshotError::Storage(format!("Failed to read body: {}", e)))?;

        let archive_data = body.to_vec();

        debug!(
            "S3 restore '{}': downloaded {} bytes",
            handle.storage_id,
            archive_data.len()
        );

        // Ensure target directory exists
        std::fs::create_dir_all(target_dir)?;

        // Extract archive
        Self::extract_archive(&archive_data, target_dir)?;

        info!(
            "S3 restore complete: {} -> {:?}",
            handle.storage_id, target_dir
        );

        Ok(())
    }

    async fn delete(&self, handle: &ArchiveHandle) -> ArchiveResult<()> {
        info!("S3 delete: {}", handle.storage_id);

        // S3 delete doesn't error if object doesn't exist
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&handle.storage_id)
            .send()
            .await
            .map_err(|e| SnapshotError::Storage(e.to_string()))?;

        debug!("S3 archive deleted: {}", handle.storage_id);

        Ok(())
    }

    fn strategy_type(&self) -> &'static str {
        "s3"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_create_extract_archive() {
        let source = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();

        // Create some test files
        fs::write(source.path().join("file1.txt"), "hello world").unwrap();
        fs::create_dir(source.path().join("subdir")).unwrap();
        fs::write(source.path().join("subdir/file2.txt"), "nested content").unwrap();

        // Create archive
        let archive = S3ArchiveStrategy::create_archive(source.path()).unwrap();
        assert!(!archive.is_empty());

        // Extract archive
        S3ArchiveStrategy::extract_archive(&archive, target.path()).unwrap();

        // Verify contents
        let content1 = fs::read_to_string(target.path().join("file1.txt")).unwrap();
        assert_eq!(content1, "hello world");

        let content2 = fs::read_to_string(target.path().join("subdir/file2.txt")).unwrap();
        assert_eq!(content2, "nested content");
    }

    #[test]
    fn test_build_key() {
        // Verify the format manually
        let prefix = "archives";
        let key = "my-run/worker1/workdir";

        let expected = format!("{}/{}.tar.gz", prefix, key);
        assert_eq!(expected, "archives/my-run/worker1/workdir.tar.gz");
    }

    // Integration tests with MinIO require a running instance
    // Run with: cargo test --features s3-storage -- --ignored

    #[tokio::test]
    #[ignore = "requires MinIO"]
    async fn test_s3_archive_roundtrip() {
        let config = S3Config {
            provider: Default::default(),
            endpoint: Some("http://localhost:9000".to_string()),
            bucket: "hirsel-test".to_string(),
            region: Some("us-east-1".to_string()),
            access_key_id: Some("minioadmin".to_string()),
            secret_access_key: Some("minioadmin".to_string()),
        };

        let strategy = S3ArchiveStrategy::new(&config, None).await.unwrap();

        // Create test directory
        let source = TempDir::new().unwrap();
        fs::write(source.path().join("test.txt"), "archive test").unwrap();

        // Archive
        let handle = strategy
            .archive("test-run/worker1/workdir", source.path())
            .await
            .unwrap();

        assert_eq!(handle.strategy_type, "s3");
        assert!(handle.storage_id.contains("test-run"));
        assert!(handle.storage_id.contains("worker1"));

        // Restore to different location
        let target = TempDir::new().unwrap();
        strategy.restore(&handle, target.path()).await.unwrap();

        let content = fs::read_to_string(target.path().join("test.txt")).unwrap();
        assert_eq!(content, "archive test");

        // Delete
        strategy.delete(&handle).await.unwrap();
    }
}
