//! Draft workspace types
//!
//! Types for managing draft workspaces and their initialization.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Starting point for a draft workspace - defines how the workspace is initialized
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StartingPoint {
    /// Fresh start with empty project (git init)
    Greenfield,
    /// Copy from local folder
    LocalFolder { path: String },
    /// Clone from git repository
    GitRepo { url: String, branch: Option<String> },
}

impl Default for StartingPoint {
    fn default() -> Self {
        Self::Greenfield
    }
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
