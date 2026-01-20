//! Local filesystem storage implementation.
//!
//! This module provides a `FileStorage` implementation that stores files
//! on the local filesystem, typically under `~/.hirsel/`.

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use walkdir::WalkDir;

use super::{FileStorage, StorageError, StorageResult};

/// Local filesystem storage backend.
///
/// Stores files relative to a root directory (typically `~/.hirsel/`).
#[derive(Debug, Clone)]
pub struct LocalFileStorage {
    root: PathBuf,
}

impl LocalFileStorage {
    /// Create a new local storage with the given root directory.
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// Get the full path for a relative path.
    fn full_path(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    /// Get the root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[async_trait]
impl FileStorage for LocalFileStorage {
    async fn read(&self, path: &str) -> StorageResult<Vec<u8>> {
        let full_path = self.full_path(path);
        fs::read(&full_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(path.to_string())
            } else {
                StorageError::Io(e)
            }
        })
    }

    async fn write(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        let full_path = self.full_path(path);

        // Create parent directories if they don't exist
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(&full_path, data).await?;
        Ok(())
    }

    async fn delete(&self, path: &str) -> StorageResult<()> {
        let full_path = self.full_path(path);
        fs::remove_file(&full_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(path.to_string())
            } else {
                StorageError::Io(e)
            }
        })
    }

    async fn exists(&self, path: &str) -> StorageResult<bool> {
        let full_path = self.full_path(path);
        Ok(full_path.exists())
    }

    async fn list(&self, prefix: &str) -> StorageResult<Vec<String>> {
        let full_path = self.full_path(prefix);

        // If the prefix doesn't exist, return empty list
        if !full_path.exists() {
            return Ok(Vec::new());
        }

        let mut files = Vec::new();

        // Use walkdir for recursive listing
        for entry in WalkDir::new(&full_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                // Convert to relative path from root
                if let Ok(relative) = entry.path().strip_prefix(&self.root) {
                    files.push(relative.to_string_lossy().to_string());
                }
            }
        }

        files.sort();
        Ok(files)
    }

    async fn create_dir(&self, path: &str) -> StorageResult<()> {
        let full_path = self.full_path(path);
        fs::create_dir_all(&full_path).await?;
        Ok(())
    }

    async fn copy(&self, from: &str, to: &str) -> StorageResult<()> {
        let from_path = self.full_path(from);
        let to_path = self.full_path(to);

        // Create parent directories for destination
        if let Some(parent) = to_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::copy(&from_path, &to_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(from.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_full_path() {
        let storage = LocalFileStorage::new("/home/test/.hirsel");
        assert_eq!(
            storage.full_path("runs/myrun/spec.md"),
            PathBuf::from("/home/test/.hirsel/runs/myrun/spec.md")
        );
    }

    #[tokio::test]
    async fn test_read_write() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        storage.write("test.txt", b"hello").await.unwrap();
        let content = storage.read("test.txt").await.unwrap();
        assert_eq!(content, b"hello");
    }

    #[tokio::test]
    async fn test_read_not_found() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        let result = storage.read("nonexistent.txt").await;
        assert!(matches!(result, Err(StorageError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_write_creates_dirs() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        storage
            .write("a/b/c/deep.txt", b"deep content")
            .await
            .unwrap();
        assert!(temp.path().join("a/b/c/deep.txt").exists());
    }

    #[tokio::test]
    async fn test_delete() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        storage.write("delete_me.txt", b"bye").await.unwrap();
        assert!(storage.exists("delete_me.txt").await.unwrap());

        storage.delete("delete_me.txt").await.unwrap();
        assert!(!storage.exists("delete_me.txt").await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_not_found() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        let result = storage.delete("nonexistent.txt").await;
        assert!(matches!(result, Err(StorageError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_list_files() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        storage.write("dir/file1.txt", b"1").await.unwrap();
        storage.write("dir/file2.txt", b"2").await.unwrap();
        storage.write("dir/sub/file3.txt", b"3").await.unwrap();

        let files = storage.list("dir/").await.unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.contains(&"dir/file1.txt".to_string()));
        assert!(files.contains(&"dir/file2.txt".to_string()));
        assert!(files.contains(&"dir/sub/file3.txt".to_string()));
    }

    #[tokio::test]
    async fn test_list_empty_prefix() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        let files = storage.list("nonexistent/").await.unwrap();
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn test_create_dir() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        storage.create_dir("new/nested/dir").await.unwrap();
        assert!(temp.path().join("new/nested/dir").exists());
        assert!(temp.path().join("new/nested/dir").is_dir());
    }

    #[tokio::test]
    async fn test_copy() {
        let temp = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp.path());

        storage.write("source.txt", b"copy me").await.unwrap();
        storage.copy("source.txt", "dest/copied.txt").await.unwrap();

        let content = storage.read("dest/copied.txt").await.unwrap();
        assert_eq!(content, b"copy me");

        // Source should still exist
        assert!(storage.exists("source.txt").await.unwrap());
    }
}
