//! Local filesystem workspace provider
//!
//! Implements WorkspaceProvider for local filesystem storage.

use async_trait::async_trait;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tracing::info;

use super::types::{FileEntry, StartingPoint, WorkspaceInfo};
use super::workspace::WorkspaceProvider;
use crate::backend::config;
use crate::backend::error::{HirselError, HirselResult};
use crate::backend::git::{clone_remote_with_branch, get_current_branch};

fn init_git_repo(path: &std::path::Path) -> HirselResult<()> {
    let repo = git2::Repository::init(path)?;
    let sig = git2::Signature::now("hirsel", "hirsel@localhost")?;
    let tree_id = {
        let mut index = repo.index()?;
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
        index.write()?;
        index.write_tree()?
    };
    let tree = repo.find_tree(tree_id)?;
    let commit_id = repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;
    let commit = repo.find_commit(commit_id)?;
    repo.branch("main", &commit, true)?;
    repo.set_head("refs/heads/main")?;
    Ok(())
}

/// Local filesystem workspace provider
///
/// Stores workspaces in ~/.hirsel/workspaces/{workspace_name}/workspace/
pub struct LocalWorkspaceProvider {
    base_dir: PathBuf,
}

impl LocalWorkspaceProvider {
    /// Create a new LocalWorkspaceProvider with the default workspaces directory
    pub fn new() -> Self {
        Self {
            base_dir: config::workspaces_dir(),
        }
    }

    /// Create a LocalWorkspaceProvider with a custom base directory
    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Copy a directory recursively
    fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> HirselResult<()> {
        if Self::copy_dir_reflink(src, dst).is_ok() {
            return Ok(());
        }

        if !dst.exists() {
            fs::create_dir_all(dst)?;
        }

        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if file_type.is_dir() {
                Self::copy_dir_recursive(&src_path, &dst_path)?;
            } else if file_type.is_file() {
                fs::copy(&src_path, &dst_path)?;
            } else if file_type.is_symlink() {
                // Copy symlink target
                let target = fs::read_link(&src_path)?;
                #[cfg(unix)]
                std::os::unix::fs::symlink(&target, &dst_path)?;
                #[cfg(windows)]
                {
                    if target.is_dir() {
                        std::os::windows::fs::symlink_dir(&target, &dst_path)?;
                    } else {
                        std::os::windows::fs::symlink_file(&target, &dst_path)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn copy_dir_reflink(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
        if dst.exists() {
            fs::remove_dir_all(dst)?;
        }
        fs::create_dir_all(dst)?;

        let status = Command::new("cp")
            .arg("-a")
            .arg("--reflink=auto")
            .arg(format!("{}/.", src.display()))
            .arg(dst)
            .status()?;

        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other("cp --reflink=auto failed"))
        }
    }
}

impl Default for LocalWorkspaceProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WorkspaceProvider for LocalWorkspaceProvider {
    async fn init(
        &self,
        workspace_name: &str,
        starting_point: &StartingPoint,
    ) -> HirselResult<WorkspaceInfo> {
        let workspace_dir = self.workspace_path(workspace_name);
        if let Some(parent) = workspace_dir.parent() {
            fs::create_dir_all(parent)?;
        }

        info!(
            "Initializing workspace '{}' at {:?}",
            workspace_name, workspace_dir
        );

        match starting_point {
            StartingPoint::Greenfield => {
                info!("Creating greenfield workspace with git init");
                init_git_repo(&workspace_dir)
                    .map_err(|e| HirselError::GitOp(format!("Failed to init git: {}", e)))?;
            }
            StartingPoint::LocalFolder { path } => {
                info!("Copying local folder from {} to workspace", path);
                let src_path = std::path::Path::new(path);
                if !src_path.exists() {
                    return Err(HirselError::NotFound(format!(
                        "Source folder does not exist: {}",
                        path
                    )));
                }
                Self::copy_dir_recursive(src_path, &workspace_dir)?;

                // Initialize git if not already a git repo
                if !workspace_dir.join(".git").exists() {
                    info!("Source folder is not a git repo, initializing git");
                    init_git_repo(&workspace_dir)
                        .map_err(|e| HirselError::GitOp(format!("Failed to init git: {}", e)))?;
                }
            }
            StartingPoint::GitRepo { url, branch } => {
                info!("Cloning git repo {} to workspace", url);
                if workspace_dir.exists() {
                    if workspace_dir.read_dir()?.next().is_some() {
                        return Err(HirselError::GitOp(format!(
                            "Workspace already exists and is not empty: {}",
                            workspace_dir.display()
                        )));
                    }
                    fs::remove_dir_all(&workspace_dir)?;
                }
                clone_remote_with_branch(url, &workspace_dir, branch.as_deref())
                    .map_err(|e| HirselError::GitOp(e.to_string()))?;
            }
        }

        let is_git = workspace_dir.join(".git").exists();
        let default_branch = if is_git {
            get_current_branch(&workspace_dir).ok()
        } else {
            None
        };

        Ok(WorkspaceInfo {
            path: workspace_dir,
            is_git,
            default_branch,
        })
    }

    fn workspace_path(&self, workspace_name: &str) -> PathBuf {
        self.base_dir.join(workspace_name).join("workspace")
    }

    async fn read_file(&self, workspace_name: &str, path: &str) -> HirselResult<Vec<u8>> {
        let full_path = self.workspace_path(workspace_name).join(path);
        fs::read(&full_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                HirselError::FileNotFound(path.to_string())
            } else {
                HirselError::Io(e)
            }
        })
    }

    async fn write_file(
        &self,
        workspace_name: &str,
        path: &str,
        content: &[u8],
    ) -> HirselResult<()> {
        let full_path = self.workspace_path(workspace_name).join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&full_path, content)?;
        Ok(())
    }

    async fn list_files(&self, workspace_name: &str, path: &str) -> HirselResult<Vec<FileEntry>> {
        let dir = self.workspace_path(workspace_name).join(path);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            let name = entry.file_name().to_string_lossy().to_string();

            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());

            entries.push(FileEntry {
                name,
                is_dir: metadata.is_dir(),
                size: if metadata.is_file() {
                    metadata.len()
                } else {
                    0
                },
                modified,
            });
        }

        // Sort by name
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    async fn exists(&self, workspace_name: &str) -> bool {
        self.workspace_path(workspace_name).exists()
    }

    async fn delete(&self, workspace_name: &str) -> HirselResult<()> {
        let path = self.workspace_path(workspace_name);
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_greenfield_workspace() {
        let temp = TempDir::new().unwrap();
        let provider = LocalWorkspaceProvider::with_base_dir(temp.path().to_path_buf());

        let info = provider
            .init("test-run", &StartingPoint::Greenfield)
            .await
            .unwrap();

        assert!(info.is_git);
        assert!(info.path.exists());
        assert!(info.path.join(".git").exists());
    }

    #[tokio::test]
    async fn test_local_folder_workspace() {
        let temp = TempDir::new().unwrap();
        let src_dir = temp.path().join("source");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("test.txt"), "hello").unwrap();

        let provider = LocalWorkspaceProvider::with_base_dir(temp.path().to_path_buf());

        let info = provider
            .init(
                "test-run",
                &StartingPoint::LocalFolder {
                    path: src_dir.to_string_lossy().to_string(),
                },
            )
            .await
            .unwrap();

        assert!(info.is_git);
        assert!(info.path.join("test.txt").exists());
    }

    #[tokio::test]
    async fn test_workspace_file_operations() {
        let temp = TempDir::new().unwrap();
        let provider = LocalWorkspaceProvider::with_base_dir(temp.path().to_path_buf());

        provider
            .init("test-run", &StartingPoint::Greenfield)
            .await
            .unwrap();

        // Write a file
        provider
            .write_file("test-run", "test.txt", b"hello world")
            .await
            .unwrap();

        // Read it back
        let content = provider.read_file("test-run", "test.txt").await.unwrap();
        assert_eq!(content, b"hello world");

        // List files
        let files = provider.list_files("test-run", ".").await.unwrap();
        assert!(files.iter().any(|f| f.name == "test.txt"));

        // Check exists
        assert!(provider.exists("test-run").await);

        // Delete
        provider.delete("test-run").await.unwrap();
        assert!(!provider.exists("test-run").await);
    }
}
