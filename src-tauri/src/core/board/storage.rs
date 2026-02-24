//! Board Storage abstraction
//!
//! Provides local and remote storage for board task files.
//! Each top-level task gets its own JSON file in the board directory.

use std::path::PathBuf;

use async_trait::async_trait;

use super::types::TaskFile;
use super::BoardError;
use crate::core::config::hirsel_dir;
use crate::core::http_client::AuthenticatedClient;

pub type StorageResult<T> = Result<T, BoardError>;

/// Trait for board file storage operations
#[async_trait]
pub trait BoardStorage: Send + Sync {
    /// Get the board directory path
    fn board_dir(&self) -> PathBuf;

    /// Read a task file by slug
    async fn read_task_file(&self, slug: &str) -> StorageResult<Option<TaskFile>>;

    /// Write a task file
    async fn write_task_file(&self, slug: &str, file: &TaskFile) -> StorageResult<()>;

    /// Delete a task file
    async fn delete_task_file(&self, slug: &str) -> StorageResult<()>;

    /// List all task file slugs
    async fn list_task_files(&self) -> StorageResult<Vec<String>>;
}

/// Local filesystem storage for board files
pub struct LocalBoardStorage {
    board_dir: PathBuf,
}

impl LocalBoardStorage {
    /// Create a new local board storage for a project
    pub fn new(project_id: i64) -> Self {
        let board_dir = hirsel_dir()
            .join("projects")
            .join(project_id.to_string())
            .join("board");
        Self { board_dir }
    }

    /// Ensure the board directory exists
    fn ensure_dir(&self) -> StorageResult<()> {
        if !self.board_dir.exists() {
            std::fs::create_dir_all(&self.board_dir)?;
        }
        Ok(())
    }

    /// Get path for a task file
    fn task_file_path(&self, slug: &str) -> PathBuf {
        self.board_dir.join(format!("{}.json", slug))
    }
}

#[async_trait]
impl BoardStorage for LocalBoardStorage {
    fn board_dir(&self) -> PathBuf {
        self.board_dir.clone()
    }

    async fn read_task_file(&self, slug: &str) -> StorageResult<Option<TaskFile>> {
        let path = self.task_file_path(slug);
        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&path)?;
        let task_file: TaskFile = serde_json::from_str(&content)?;
        Ok(Some(task_file))
    }

    async fn write_task_file(&self, slug: &str, file: &TaskFile) -> StorageResult<()> {
        self.ensure_dir()?;
        let path = self.task_file_path(slug);
        let content = serde_json::to_string_pretty(file)?;
        std::fs::write(&path, content.as_bytes())?;
        Ok(())
    }

    async fn delete_task_file(&self, slug: &str) -> StorageResult<()> {
        let path = self.task_file_path(slug);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    async fn list_task_files(&self) -> StorageResult<Vec<String>> {
        self.ensure_dir()?;

        let mut slugs = Vec::new();
        for entry in std::fs::read_dir(&self.board_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Some(stem) = path.file_stem() {
                    let slug = stem.to_string_lossy().to_string();
                    slugs.push(slug);
                }
            }
        }
        Ok(slugs)
    }
}

/// Remote HTTP storage for board files
pub struct RemoteBoardStorage {
    project_id: i64,
    client: AuthenticatedClient,
    /// Cached board directory (fetched from server)
    board_dir: PathBuf,
}

impl RemoteBoardStorage {
    /// Create a new remote board storage
    pub fn new(project_id: i64, url: &str, api_key: &str) -> Self {
        Self {
            project_id,
            client: AuthenticatedClient::new(url, api_key),
            board_dir: hirsel_dir()
                .join("projects")
                .join(project_id.to_string())
                .join("board"),
        }
    }
}

#[async_trait]
impl BoardStorage for RemoteBoardStorage {
    fn board_dir(&self) -> PathBuf {
        self.board_dir.clone()
    }

    async fn read_task_file(&self, slug: &str) -> StorageResult<Option<TaskFile>> {
        let result: Result<TaskFile, _> = self
            .client
            .get(&format!("/api/board/{}/tasks/{}", self.project_id, slug))
            .await;

        match result {
            Ok(file) => Ok(Some(file)),
            Err(e) => {
                // Check if it's a 404
                if e.to_string().contains("404") {
                    Ok(None)
                } else {
                    Err(BoardError::Http(e))
                }
            }
        }
    }

    async fn write_task_file(&self, slug: &str, file: &TaskFile) -> StorageResult<()> {
        self.client
            .post_empty(
                &format!("/api/board/{}/tasks/{}", self.project_id, slug),
                file,
            )
            .await?;
        Ok(())
    }

    async fn delete_task_file(&self, slug: &str) -> StorageResult<()> {
        self.client
            .delete(&format!("/api/board/{}/tasks/{}", self.project_id, slug))
            .await?;
        Ok(())
    }

    async fn list_task_files(&self) -> StorageResult<Vec<String>> {
        let slugs: Vec<String> = self
            .client
            .get(&format!("/api/board/{}/tasks", self.project_id))
            .await?;
        Ok(slugs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_local_storage_crud() {
        let dir = tempdir().unwrap();
        let storage = LocalBoardStorage {
            board_dir: dir.path().to_path_buf(),
        };

        // Initially empty
        let files = storage.list_task_files().await.unwrap();
        assert!(files.is_empty());

        // Write a task file
        let task_file = TaskFile {
            task: super::super::TaskTree {
                id: "build-api".to_string(),
                name: "Build API".to_string(),
                status: super::super::TaskStatus::Todo,
                content: "Build the API".to_string(),
                children: vec![],
                x: None,
                y: None,
                validated: None,
            },
            evals: vec![],
        };

        storage
            .write_task_file("build-api", &task_file)
            .await
            .unwrap();

        // List shows the file
        let files = storage.list_task_files().await.unwrap();
        assert_eq!(files, vec!["build-api"]);

        // Read it back
        let read_back = storage.read_task_file("build-api").await.unwrap().unwrap();
        assert_eq!(read_back.task.id, "build-api");
        assert_eq!(read_back.task.name, "Build API");

        // Delete it
        storage.delete_task_file("build-api").await.unwrap();
        let files = storage.list_task_files().await.unwrap();
        assert!(files.is_empty());

        // Reading deleted file returns None
        assert!(storage.read_task_file("build-api").await.unwrap().is_none());
    }
}
