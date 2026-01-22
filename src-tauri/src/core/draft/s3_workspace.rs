//! S3-compatible workspace provider
//!
//! Implements WorkspaceProvider for S3-compatible storage (MinIO, Tigris, AWS S3).

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use super::types::{FileEntry, StartingPoint, WorkspaceInfo};
use super::workspace::WorkspaceProvider;
use crate::core::error::{HirselError, HirselResult};
use crate::core::git::clone_remote_with_branch;
use crate::core::ops::init_git_repo;
use crate::core::storage::FileStorage;

/// S3-compatible workspace provider
///
/// Stores workspaces in S3 under workspaces/{run_name}/
#[cfg(feature = "s3-storage")]
pub struct S3WorkspaceProvider {
    storage: Arc<dyn FileStorage>,
    bucket_prefix: String,
}

#[cfg(feature = "s3-storage")]
impl S3WorkspaceProvider {
    /// Create a new S3WorkspaceProvider with the given storage backend
    pub fn new(storage: Arc<dyn FileStorage>) -> Self {
        Self {
            storage,
            bucket_prefix: "workspaces".to_string(),
        }
    }

    /// Create a new S3WorkspaceProvider with a custom prefix
    pub fn with_prefix(storage: Arc<dyn FileStorage>, prefix: impl Into<String>) -> Self {
        Self {
            storage,
            bucket_prefix: prefix.into(),
        }
    }

    /// Get the S3 prefix for a workspace
    fn workspace_prefix(&self, run_name: &str) -> String {
        format!("{}/{}", self.bucket_prefix, run_name)
    }

    /// Upload a local directory to S3
    async fn upload_directory(&self, src: &std::path::Path, prefix: &str) -> HirselResult<()> {
        use walkdir::WalkDir;

        for entry in WalkDir::new(src) {
            let entry = entry.map_err(|e| HirselError::Internal(format!("Walk error: {}", e)))?;
            let path = entry.path();

            if path.is_file() {
                let relative = path
                    .strip_prefix(src)
                    .map_err(|e| HirselError::Internal(format!("Path error: {}", e)))?;
                let key = format!("{}/{}", prefix, relative.to_string_lossy());

                let content = std::fs::read(path)?;
                self.storage
                    .write(&key, &content)
                    .await
                    .map_err(|e| HirselError::Internal(format!("Storage write error: {}", e)))?;
            }
        }

        Ok(())
    }
}

#[cfg(feature = "s3-storage")]
#[async_trait]
impl WorkspaceProvider for S3WorkspaceProvider {
    async fn init(
        &self,
        run_name: &str,
        starting_point: &StartingPoint,
    ) -> HirselResult<WorkspaceInfo> {
        let prefix = self.workspace_prefix(run_name);

        info!(
            "Initializing S3 workspace for run '{}' at prefix '{}'",
            run_name, prefix
        );

        match starting_point {
            StartingPoint::Greenfield => {
                // Create a minimal local repo, then upload
                let temp_dir = tempfile::tempdir()?;
                init_git_repo(temp_dir.path())
                    .map_err(|e| HirselError::GitOp(format!("Failed to init git: {}", e)))?;
                self.upload_directory(temp_dir.path(), &prefix).await?;
            }
            StartingPoint::LocalFolder { path } => {
                let src_path = std::path::Path::new(path);
                if !src_path.exists() {
                    return Err(HirselError::NotFound(format!(
                        "Source folder does not exist: {}",
                        path
                    )));
                }

                // If not a git repo, init locally first
                let upload_path = if !src_path.join(".git").exists() {
                    let temp_dir = tempfile::tempdir()?;
                    // Copy files to temp
                    copy_dir_contents(src_path, temp_dir.path())?;
                    init_git_repo(temp_dir.path())
                        .map_err(|e| HirselError::GitOp(format!("Failed to init git: {}", e)))?;
                    self.upload_directory(temp_dir.path(), &prefix).await?;
                    temp_dir.path().to_path_buf()
                } else {
                    self.upload_directory(src_path, &prefix).await?;
                    src_path.to_path_buf()
                };

                info!("Uploaded local folder to S3 from {:?}", upload_path);
            }
            StartingPoint::GitRepo { url, branch } => {
                // Clone to temp, then upload to S3
                let temp_dir = tempfile::tempdir()?;
                clone_remote_with_branch(url, temp_dir.path(), branch.as_deref())
                    .map_err(|e| HirselError::GitOp(e.to_string()))?;
                self.upload_directory(temp_dir.path(), &prefix).await?;
            }
        }

        Ok(WorkspaceInfo {
            path: PathBuf::from(&prefix),
            is_git: true, // We always init as git
            default_branch: Some("main".to_string()),
        })
    }

    fn workspace_path(&self, run_name: &str) -> PathBuf {
        PathBuf::from(self.workspace_prefix(run_name))
    }

    async fn read_file(&self, run_name: &str, path: &str) -> HirselResult<Vec<u8>> {
        let key = format!("{}/{}", self.workspace_prefix(run_name), path);
        self.storage
            .read(&key)
            .await
            .map_err(|e| HirselError::Internal(format!("Storage read error: {}", e)))
    }

    async fn write_file(&self, run_name: &str, path: &str, content: &[u8]) -> HirselResult<()> {
        let key = format!("{}/{}", self.workspace_prefix(run_name), path);
        self.storage
            .write(&key, content)
            .await
            .map_err(|e| HirselError::Internal(format!("Storage write error: {}", e)))
    }

    async fn list_files(&self, run_name: &str, path: &str) -> HirselResult<Vec<FileEntry>> {
        let prefix = if path.is_empty() || path == "." {
            format!("{}/", self.workspace_prefix(run_name))
        } else {
            format!("{}/{}/", self.workspace_prefix(run_name), path)
        };

        let files = self
            .storage
            .list(&prefix)
            .await
            .map_err(|e| HirselError::Internal(format!("Storage list error: {}", e)))?;

        // Convert S3 keys to FileEntry
        let entries: Vec<FileEntry> = files
            .into_iter()
            .filter_map(|key| {
                // Extract filename from key
                let relative = key.strip_prefix(&prefix)?;
                if relative.is_empty() {
                    return None;
                }

                // Check if this is a direct child or nested
                let parts: Vec<&str> = relative.split('/').collect();
                if parts.is_empty() {
                    return None;
                }

                let name = parts[0].to_string();
                let is_dir = parts.len() > 1;

                Some(FileEntry {
                    name,
                    is_dir,
                    size: 0, // S3 doesn't easily provide size in list
                    modified: None,
                })
            })
            .collect();

        // Deduplicate (directories may appear multiple times)
        let mut seen = std::collections::HashSet::new();
        let unique: Vec<FileEntry> = entries
            .into_iter()
            .filter(|e| seen.insert(e.name.clone()))
            .collect();

        Ok(unique)
    }

    async fn exists(&self, run_name: &str) -> bool {
        let prefix = format!("{}/", self.workspace_prefix(run_name));
        match self.storage.list(&prefix).await {
            Ok(files) => !files.is_empty(),
            Err(_) => false,
        }
    }

    async fn delete(&self, run_name: &str) -> HirselResult<()> {
        let prefix = format!("{}/", self.workspace_prefix(run_name));
        let files = self
            .storage
            .list(&prefix)
            .await
            .map_err(|e| HirselError::Internal(format!("Storage list error: {}", e)))?;

        for key in files {
            self.storage
                .delete(&key)
                .await
                .map_err(|e| HirselError::Internal(format!("Storage delete error: {}", e)))?;
        }

        Ok(())
    }
}

/// Helper function to copy directory contents
#[cfg(feature = "s3-storage")]
fn copy_dir_contents(src: &std::path::Path, dst: &std::path::Path) -> HirselResult<()> {
    use std::fs;

    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_contents(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}
