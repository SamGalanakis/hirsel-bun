//! Storage abstraction layer for hirsel.
//!
//! This module provides a unified interface for file storage operations,
//! supporting multiple backends:
//!
//! - **LocalFileStorage**: Filesystem storage (default)
//! - **S3FileStorage**: S3-compatible object storage (MinIO, Tigris, AWS S3)
//!
//! # Usage
//!
//! ```rust,ignore
//! use hirsel_lib::core::storage::{create_file_storage, FileStorage};
//!
//! // Create storage from config
//! let storage = create_file_storage(&config.storage).await?;
//!
//! // Read a file
//! let content = storage.read("runs/myrun/spec.md").await?;
//!
//! // Write a file
//! storage.write("runs/myrun/spec.md", b"# My Spec").await?;
//! ```

mod local;
#[cfg(feature = "s3-storage")]
mod s3;
#[cfg(feature = "s3-storage")]
mod s3_client;

pub use local::LocalFileStorage;
#[cfg(feature = "s3-storage")]
pub use s3::S3FileStorage;
#[cfg(feature = "s3-storage")]
pub use s3_client::S3ClientFactory;

use async_trait::async_trait;
use std::path::Path;
use thiserror::Error;

use crate::core::config::{StorageBackend, StorageConfig};

/// Storage errors
#[derive(Debug, Error)]
pub enum StorageError {
    /// File or object not found
    #[error("Not found: {0}")]
    NotFound(String),

    /// I/O error during local filesystem operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// S3-specific error
    #[error("S3 error: {0}")]
    S3(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// UTF-8 encoding error
    #[error("UTF-8 encoding error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Result type for storage operations
pub type StorageResult<T> = Result<T, StorageError>;

/// File storage trait for abstracting storage backends.
///
/// Paths are relative to the storage root (e.g., `runs/myrun/spec.md`).
/// For local storage, this is relative to `~/.hirsel/`.
/// For S3, this is the object key within the bucket.
#[async_trait]
pub trait FileStorage: Send + Sync {
    /// Read a file's contents as bytes.
    async fn read(&self, path: &str) -> StorageResult<Vec<u8>>;

    /// Write data to a file, creating parent directories if needed.
    async fn write(&self, path: &str, data: &[u8]) -> StorageResult<()>;

    /// Delete a file.
    async fn delete(&self, path: &str) -> StorageResult<()>;

    /// Check if a file exists.
    async fn exists(&self, path: &str) -> StorageResult<bool>;

    /// List files with a given prefix.
    ///
    /// Returns relative paths within the storage.
    async fn list(&self, prefix: &str) -> StorageResult<Vec<String>>;

    /// Read a file's contents as a UTF-8 string.
    async fn read_string(&self, path: &str) -> StorageResult<String> {
        let bytes = self.read(path).await?;
        String::from_utf8(bytes).map_err(StorageError::from)
    }

    /// Write a string to a file.
    async fn write_string(&self, path: &str, content: &str) -> StorageResult<()> {
        self.write(path, content.as_bytes()).await
    }

    /// Create a directory (no-op for S3, creates on local filesystem).
    async fn create_dir(&self, path: &str) -> StorageResult<()>;

    /// Copy a file from one path to another.
    async fn copy(&self, from: &str, to: &str) -> StorageResult<()> {
        let data = self.read(from).await?;
        self.write(to, &data).await
    }
}

/// Create a file storage instance based on configuration.
///
/// This is the main factory function for creating storage backends.
pub async fn create_file_storage(config: &StorageConfig) -> StorageResult<Box<dyn FileStorage>> {
    match config.files {
        StorageBackend::Local => {
            let root = crate::core::config::hirsel_dir();
            Ok(Box::new(LocalFileStorage::new(root)))
        }
        StorageBackend::S3 => {
            #[cfg(feature = "s3-storage")]
            {
                // Get the storage name (default_storage or first available)
                let storage_name = config
                    .default_storage
                    .as_ref()
                    .or_else(|| config.storages.keys().next())
                    .ok_or_else(|| {
                        StorageError::Config(
                            "S3 storage requires [storage.storages.<name>] configuration".into(),
                        )
                    })?;

                let s3_config = config.storages.get(storage_name).ok_or_else(|| {
                    StorageError::Config(format!(
                        "Storage '{}' not found in [storage.storages]",
                        storage_name
                    ))
                })?;
                let storage = S3FileStorage::new(s3_config).await?;
                Ok(Box::new(storage))
            }
            #[cfg(not(feature = "s3-storage"))]
            {
                Err(StorageError::Config(
                    "S3 storage is not enabled. Rebuild with --features s3-storage".into(),
                ))
            }
        }
    }
}

/// Create a local file storage instance with the given root directory.
///
/// This is useful for tests or when you need a specific root directory.
pub fn create_local_storage<P: AsRef<Path>>(root: P) -> LocalFileStorage {
    LocalFileStorage::new(root)
}

/// Create a local file storage instance with the default hirsel root.
pub fn create_default_local_storage() -> LocalFileStorage {
    let root = crate::core::config::hirsel_dir();
    LocalFileStorage::new(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_local_storage_roundtrip() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        // Write and read
        storage.write("test.txt", b"hello world").await.unwrap();
        let content = storage.read("test.txt").await.unwrap();
        assert_eq!(content, b"hello world");

        // Exists
        assert!(storage.exists("test.txt").await.unwrap());
        assert!(!storage.exists("nonexistent.txt").await.unwrap());

        // Delete
        storage.delete("test.txt").await.unwrap();
        assert!(!storage.exists("test.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_local_storage_nested_paths() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        // Write to nested path (should create directories)
        storage
            .write("a/b/c/file.txt", b"nested content")
            .await
            .unwrap();

        let content = storage.read("a/b/c/file.txt").await.unwrap();
        assert_eq!(content, b"nested content");
    }

    #[tokio::test]
    async fn test_local_storage_list() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        // Create some files
        storage.write("runs/run1/spec.md", b"spec1").await.unwrap();
        storage.write("runs/run1/eval.md", b"eval1").await.unwrap();
        storage.write("runs/run2/spec.md", b"spec2").await.unwrap();

        // List all runs
        let files = storage.list("runs/run1/").await.unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.contains(&"runs/run1/spec.md".to_string()));
        assert!(files.contains(&"runs/run1/eval.md".to_string()));
    }

    #[tokio::test]
    async fn test_local_storage_string_helpers() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        storage.write_string("test.md", "# Hello").await.unwrap();
        let content = storage.read_string("test.md").await.unwrap();
        assert_eq!(content, "# Hello");
    }
}
