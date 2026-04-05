//! Draft workspace types
//!
//! Types for managing draft workspaces and their initialization.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use surrealdb::types::SurrealValue;

/// Starting point for a draft workspace - defines how the workspace is initialized
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
#[serde(tag = "type", rename_all = "camelCase")]
#[derive(Default)]
pub enum StartingPoint {
    /// Fresh start with empty project (git init)
    #[default]
    Greenfield,
    /// Copy from local folder
    LocalFolder { path: String },
    /// Clone from git repository
    GitRepo { url: String, branch: Option<String> },
}

impl StartingPoint {
    /// Get the type name as a string (for serialization/logging).
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Greenfield => "greenfield",
            Self::LocalFolder { .. } => "local_folder",
            Self::GitRepo { .. } => "git_repo",
        }
    }

    /// Get the local filesystem path if this is a LocalFolder starting point.
    pub fn local_path(&self) -> Option<PathBuf> {
        match self {
            Self::LocalFolder { path } => Some(PathBuf::from(path)),
            _ => None,
        }
    }

    /// Get the git remote URL if this is a GitRepo starting point.
    pub fn git_url(&self) -> Option<&str> {
        match self {
            Self::GitRepo { url, .. } => Some(url),
            _ => None,
        }
    }
}

/// Information about an initialized workspace
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    /// Workspace root path (local path or virtual path for remote)
    pub path: PathBuf,
    /// Whether the workspace has a .git directory
    pub is_git: bool,
    /// Current branch name if a git repo
    pub default_branch: Option<String>,
}

/// Entry in a workspace directory listing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    /// File or directory name
    pub name: String,
    /// Whether this is a directory
    pub is_dir: bool,
    /// File size in bytes (0 for directories)
    pub size: u64,
    /// Last modified timestamp (Unix epoch seconds)
    pub modified: Option<u64>,
}
