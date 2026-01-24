//! S3-compatible object storage implementation.
//!
//! This module provides a `FileStorage` implementation that stores files
//! in S3-compatible object storage (AWS S3, MinIO, Tigris, etc.).

use async_trait::async_trait;
use aws_sdk_s3::{error::SdkError, primitives::ByteStream, Client};

use super::s3_client::S3ClientFactory;
use super::{FileStorage, StorageError, StorageResult};
use crate::core::config::S3Config;

/// S3-compatible object storage backend.
///
/// Stores files in an S3 bucket. The storage paths map directly to S3 object keys.
#[derive(Debug, Clone)]
pub struct S3FileStorage {
    client: Client,
    bucket: String,
}

impl S3FileStorage {
    /// Create a new S3 storage instance from configuration.
    pub async fn new(config: &S3Config) -> StorageResult<Self> {
        let client = S3ClientFactory::create(config).await?;
        let bucket = S3ClientFactory::bucket(config)?;

        Ok(Self { client, bucket })
    }

    /// Convert S3 SDK errors to StorageError
    fn convert_error<E: std::fmt::Display>(e: SdkError<E>, path: &str) -> StorageError {
        let msg = e.to_string();
        if msg.contains("NoSuchKey") || msg.contains("NotFound") {
            StorageError::NotFound(path.to_string())
        } else {
            StorageError::S3(msg)
        }
    }
}

#[async_trait]
impl FileStorage for S3FileStorage {
    async fn read(&self, path: &str) -> StorageResult<Vec<u8>> {
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
            .map_err(|e| Self::convert_error(e, path))?;

        let bytes = resp
            .body
            .collect()
            .await
            .map_err(|e| StorageError::S3(format!("Failed to read body: {}", e)))?;

        Ok(bytes.to_vec())
    }

    async fn write(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(path)
            .body(ByteStream::from(data.to_vec()))
            .send()
            .await
            .map_err(|e| StorageError::S3(e.to_string()))?;

        Ok(())
    }

    async fn delete(&self, path: &str) -> StorageResult<()> {
        // Note: S3 delete doesn't error if the object doesn't exist
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
            .map_err(|e| StorageError::S3(e.to_string()))?;

        Ok(())
    }

    async fn exists(&self, path: &str) -> StorageResult<bool> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("NotFound") || msg.contains("NoSuchKey") {
                    Ok(false)
                } else {
                    Err(StorageError::S3(msg))
                }
            }
        }
    }

    async fn list(&self, prefix: &str) -> StorageResult<Vec<String>> {
        let mut files = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix);

            if let Some(token) = continuation_token.take() {
                request = request.continuation_token(token);
            }

            let resp = request
                .send()
                .await
                .map_err(|e| StorageError::S3(e.to_string()))?;

            if let Some(contents) = resp.contents {
                for obj in contents {
                    if let Some(key) = obj.key {
                        files.push(key);
                    }
                }
            }

            if resp.is_truncated.unwrap_or(false) {
                continuation_token = resp.next_continuation_token;
            } else {
                break;
            }
        }

        files.sort();
        Ok(files)
    }

    async fn create_dir(&self, _path: &str) -> StorageResult<()> {
        // S3 doesn't have directories - they're implicit from object keys
        // This is a no-op for S3
        Ok(())
    }

    async fn copy(&self, from: &str, to: &str) -> StorageResult<()> {
        let copy_source = format!("{}/{}", self.bucket, from);

        self.client
            .copy_object()
            .bucket(&self.bucket)
            .key(to)
            .copy_source(copy_source)
            .send()
            .await
            .map_err(|e| Self::convert_error(e, from))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // S3 tests require a running MinIO or S3 instance
    // Run with: docker run -p 9000:9000 -p 9001:9001 minio/minio server /data --console-address ":9001"
    //
    // These tests are disabled by default. Enable with:
    // cargo test --features s3-storage -- --ignored

    use super::*;

    fn get_test_config() -> S3Config {
        S3Config {
            endpoint: Some("http://localhost:9000".to_string()),
            bucket: "hirsel-test".to_string(),
            region: Some("us-east-1".to_string()),
            access_key_id: Some("minioadmin".to_string()),
            secret_access_key: Some("minioadmin".to_string()),
        }
    }

    #[tokio::test]
    #[ignore = "requires MinIO"]
    async fn test_s3_roundtrip() {
        let config = get_test_config();
        let storage = S3FileStorage::new(&config).await.unwrap();

        // Write
        storage.write("test/file.txt", b"hello s3").await.unwrap();

        // Read
        let content = storage.read("test/file.txt").await.unwrap();
        assert_eq!(content, b"hello s3");

        // Exists
        assert!(storage.exists("test/file.txt").await.unwrap());
        assert!(!storage.exists("nonexistent.txt").await.unwrap());

        // Delete
        storage.delete("test/file.txt").await.unwrap();
        assert!(!storage.exists("test/file.txt").await.unwrap());
    }

    #[tokio::test]
    #[ignore = "requires MinIO"]
    async fn test_s3_list() {
        let config = get_test_config();
        let storage = S3FileStorage::new(&config).await.unwrap();

        // Write some files
        storage.write("list-test/a.txt", b"a").await.unwrap();
        storage.write("list-test/b.txt", b"b").await.unwrap();
        storage.write("list-test/sub/c.txt", b"c").await.unwrap();

        // List
        let files = storage.list("list-test/").await.unwrap();
        assert!(files.contains(&"list-test/a.txt".to_string()));
        assert!(files.contains(&"list-test/b.txt".to_string()));
        assert!(files.contains(&"list-test/sub/c.txt".to_string()));

        // Cleanup
        storage.delete("list-test/a.txt").await.unwrap();
        storage.delete("list-test/b.txt").await.unwrap();
        storage.delete("list-test/sub/c.txt").await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires MinIO"]
    async fn test_s3_copy() {
        let config = get_test_config();
        let storage = S3FileStorage::new(&config).await.unwrap();

        storage.write("copy-src.txt", b"copy me").await.unwrap();
        storage.copy("copy-src.txt", "copy-dst.txt").await.unwrap();

        let content = storage.read("copy-dst.txt").await.unwrap();
        assert_eq!(content, b"copy me");

        // Cleanup
        storage.delete("copy-src.txt").await.unwrap();
        storage.delete("copy-dst.txt").await.unwrap();
    }
}
