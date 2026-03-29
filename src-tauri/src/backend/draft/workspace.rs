//! Workspace provider trait
//!
//! Abstracts workspace storage - works the same for local (filesystem) and remote (S3).

use async_trait::async_trait;
use std::path::PathBuf;

use super::types::{FileEntry, StartingPoint, WorkspaceInfo};
use crate::backend::error::HirselResult;

/// Workspace provider trait for abstracting workspace storage backends.
///
/// Provides a common interface for local filesystem and S3-based workspace storage.
/// The workspace is the directory where the AI agents work - it contains the project
/// files that are being developed.
#[async_trait]
pub trait WorkspaceProvider: Send + Sync {
    /// Initialize workspace from starting point
    ///
    /// Creates the workspace directory and populates it based on the starting point:
    /// - Greenfield: Creates empty git repo
    /// - LocalFolder: Copies folder contents
    /// - GitRepo: Clones repository
    async fn init(
        &self,
        runtime_name: &str,
        starting_point: &StartingPoint,
    ) -> HirselResult<WorkspaceInfo>;

    /// Get workspace root path (local path or virtual path for remote)
    fn workspace_path(&self, runtime_name: &str) -> PathBuf;

    /// Read file from workspace
    async fn read_file(&self, runtime_name: &str, path: &str) -> HirselResult<Vec<u8>>;

    /// Write file to workspace
    async fn write_file(&self, runtime_name: &str, path: &str, content: &[u8]) -> HirselResult<()>;

    /// List files in workspace directory
    async fn list_files(&self, runtime_name: &str, path: &str) -> HirselResult<Vec<FileEntry>>;

    /// Check if workspace exists
    async fn exists(&self, runtime_name: &str) -> bool;

    /// Delete workspace
    async fn delete(&self, runtime_name: &str) -> HirselResult<()>;

    /// Read file as string (convenience method)
    async fn read_file_string(&self, runtime_name: &str, path: &str) -> HirselResult<String> {
        let bytes = self.read_file(runtime_name, path).await?;
        String::from_utf8(bytes).map_err(|e| {
            crate::backend::error::HirselError::Internal(format!("Invalid UTF-8: {}", e))
        })
    }

    /// Write string to file (convenience method)
    async fn write_file_string(
        &self,
        runtime_name: &str,
        path: &str,
        content: &str,
    ) -> HirselResult<()> {
        self.write_file(runtime_name, path, content.as_bytes())
            .await
    }
}
