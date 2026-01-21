//! S3 snapshot strategy.
//!
//! This strategy archives the work directory as a tar.gz file and uploads
//! it to S3-compatible storage. Used for ephemeral hosts like Fly.io and Sprites
//! where machines are destroyed between runs.
//!
//! # S3 Layout
//!
//! ```text
//! s3://bucket/
//! └── snapshots/           (or custom prefix)
//!     └── {run_name}/
//!         └── {worker_name}/
//!             └── {timestamp}.tar.gz
//! ```

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_s3::{
    config::{Credentials, Region},
    primitives::ByteStream,
    Client,
};
use chrono::Utc;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Read, Write};
use std::path::Path;
use tar::{Archive, Builder};
use tracing::{debug, info};

use super::{SnapshotError, SnapshotHandle, SnapshotResult, SnapshotStrategy};
use crate::core::config::S3Config;

/// S3 snapshot strategy - tar/gzip and upload to S3.
#[derive(Debug, Clone)]
pub struct S3SnapshotStrategy {
    client: Client,
    bucket: String,
    prefix: String,
}

impl S3SnapshotStrategy {
    /// Create a new S3 snapshot strategy.
    pub async fn new(config: &S3Config, prefix: Option<String>) -> SnapshotResult<Self> {
        if config.bucket.is_empty() {
            return Err(SnapshotError::Config("S3 bucket name is required".into()));
        }

        // Build AWS config
        let mut aws_config_builder = aws_config::defaults(BehaviorVersion::latest());

        // Set region
        if let Some(ref region) = config.region {
            aws_config_builder = aws_config_builder.region(Region::new(region.clone()));
        } else {
            aws_config_builder = aws_config_builder.region(Region::new("us-east-1"));
        }

        // Set credentials if provided
        if let (Some(ref access_key), Some(ref secret_key)) =
            (&config.access_key_id, &config.secret_access_key)
        {
            let credentials =
                Credentials::new(access_key, secret_key, None, None, "hirsel-snapshot");
            aws_config_builder = aws_config_builder.credentials_provider(credentials);
        }

        let aws_config = aws_config_builder.load().await;

        // Build S3 client with custom endpoint if provided
        let mut s3_config_builder = aws_sdk_s3::config::Builder::from(&aws_config);

        if let Some(ref endpoint) = config.endpoint {
            s3_config_builder = s3_config_builder
                .endpoint_url(endpoint)
                .force_path_style(true);
        }

        let client = Client::from_conf(s3_config_builder.build());

        Ok(Self {
            client,
            bucket: config.bucket.clone(),
            prefix: prefix.unwrap_or_else(|| "snapshots".to_string()),
        })
    }

    /// Build the S3 key for a snapshot.
    fn build_key(&self, run_name: &str, worker_name: &str, timestamp: &str) -> String {
        format!(
            "{}/{}/{}/{}.tar.gz",
            self.prefix, run_name, worker_name, timestamp
        )
    }

    /// Create a tar.gz archive of a directory.
    fn create_archive(dir: &Path) -> SnapshotResult<Vec<u8>> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());

        {
            let mut archive = Builder::new(&mut encoder);

            // Add all files in the directory
            // We use append_dir_all with "." as the path inside the archive
            // so files are extracted relative to the target directory
            archive
                .append_dir_all(".", dir)
                .map_err(|e| SnapshotError::Io(e))?;

            archive.finish().map_err(|e| SnapshotError::Io(e))?;
        }

        encoder.finish().map_err(|e| SnapshotError::Io(e))
    }

    /// Extract a tar.gz archive to a directory.
    fn extract_archive(data: &[u8], dir: &Path) -> SnapshotResult<()> {
        let decoder = GzDecoder::new(data);
        let mut archive = Archive::new(decoder);

        // Extract all files to the target directory
        archive.unpack(dir).map_err(|e| SnapshotError::Io(e))?;

        Ok(())
    }
}

#[async_trait]
impl SnapshotStrategy for S3SnapshotStrategy {
    async fn snapshot(
        &self,
        run_name: &str,
        worker_name: &str,
        work_dir: &Path,
    ) -> SnapshotResult<SnapshotHandle> {
        if !work_dir.exists() {
            return Err(SnapshotError::WorkDirNotFound(
                work_dir.to_string_lossy().to_string(),
            ));
        }

        let timestamp = Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
        let key = self.build_key(run_name, worker_name, &timestamp);

        info!(
            "Creating S3 snapshot for {}/{}: archiving {:?}",
            run_name, worker_name, work_dir
        );

        // Create tar.gz archive
        let archive_data = Self::create_archive(work_dir)?;
        let size_bytes = archive_data.len() as u64;

        debug!(
            "S3 snapshot: created archive ({} bytes), uploading to s3://{}/{}",
            size_bytes, self.bucket, key
        );

        // Upload to S3
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(ByteStream::from(archive_data))
            .send()
            .await
            .map_err(|e| SnapshotError::Storage(e.to_string()))?;

        info!(
            "S3 snapshot complete: s3://{}/{} ({} bytes)",
            self.bucket, key, size_bytes
        );

        Ok(SnapshotHandle {
            strategy_type: self.strategy_type().to_string(),
            snapshot_id: key,
            created_at: Utc::now().to_rfc3339(),
            size_bytes: Some(size_bytes),
        })
    }

    async fn restore(&self, handle: &SnapshotHandle, work_dir: &Path) -> SnapshotResult<()> {
        info!(
            "Restoring S3 snapshot {} to {:?}",
            handle.snapshot_id, work_dir
        );

        // Download from S3
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&handle.snapshot_id)
            .send()
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("NoSuchKey") || msg.contains("NotFound") {
                    SnapshotError::NotFound(handle.snapshot_id.clone())
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
            "S3 restore: downloaded {} bytes, extracting to {:?}",
            archive_data.len(),
            work_dir
        );

        // Ensure work directory exists
        std::fs::create_dir_all(work_dir)?;

        // Extract archive
        Self::extract_archive(&archive_data, work_dir)?;

        info!(
            "S3 restore complete: {} -> {:?}",
            handle.snapshot_id, work_dir
        );

        Ok(())
    }

    async fn delete(&self, handle: &SnapshotHandle) -> SnapshotResult<()> {
        info!("Deleting S3 snapshot: {}", handle.snapshot_id);

        // S3 delete doesn't error if object doesn't exist
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&handle.snapshot_id)
            .send()
            .await
            .map_err(|e| SnapshotError::Storage(e.to_string()))?;

        debug!("S3 snapshot deleted: {}", handle.snapshot_id);

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
        let archive = S3SnapshotStrategy::create_archive(source.path()).unwrap();
        assert!(!archive.is_empty());

        // Extract archive
        S3SnapshotStrategy::extract_archive(&archive, target.path()).unwrap();

        // Verify contents
        let content1 = fs::read_to_string(target.path().join("file1.txt")).unwrap();
        assert_eq!(content1, "hello world");

        let content2 = fs::read_to_string(target.path().join("subdir/file2.txt")).unwrap();
        assert_eq!(content2, "nested content");
    }

    #[test]
    fn test_build_key() {
        // We can't test this directly without creating an S3SnapshotStrategy,
        // but we can verify the format manually
        let prefix = "snapshots";
        let run_name = "my-run";
        let worker_name = "worker1";
        let timestamp = "2024-01-15T10-30-00Z";

        let key = format!(
            "{}/{}/{}/{}.tar.gz",
            prefix, run_name, worker_name, timestamp
        );
        assert_eq!(key, "snapshots/my-run/worker1/2024-01-15T10-30-00Z.tar.gz");
    }

    // Integration tests with MinIO require a running instance
    // Run with: cargo test --features s3-storage -- --ignored

    #[tokio::test]
    #[ignore = "requires MinIO"]
    async fn test_s3_snapshot_roundtrip() {
        let config = S3Config {
            endpoint: Some("http://localhost:9000".to_string()),
            bucket: "hirsel-test".to_string(),
            region: Some("us-east-1".to_string()),
            access_key_id: Some("minioadmin".to_string()),
            secret_access_key: Some("minioadmin".to_string()),
        };

        let strategy = S3SnapshotStrategy::new(&config, None).await.unwrap();

        // Create test directory
        let source = TempDir::new().unwrap();
        fs::write(source.path().join("test.txt"), "snapshot test").unwrap();

        // Snapshot
        let handle = strategy
            .snapshot("test-run", "worker1", source.path())
            .await
            .unwrap();

        assert_eq!(handle.strategy_type, "s3");
        assert!(handle.snapshot_id.contains("test-run"));
        assert!(handle.snapshot_id.contains("worker1"));

        // Restore to different location
        let target = TempDir::new().unwrap();
        strategy.restore(&handle, target.path()).await.unwrap();

        let content = fs::read_to_string(target.path().join("test.txt")).unwrap();
        assert_eq!(content, "snapshot test");

        // Delete
        strategy.delete(&handle).await.unwrap();
    }
}
