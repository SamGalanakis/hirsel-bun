//! Forge abstraction for PR/merge operations
//!
//! Provides a trait-based abstraction over source code forges (GitHub, GitLab, etc.)
//! Currently only GitHub is implemented.

mod github;

pub use github::{parse_github_remote_url, GitHubForge};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error type for forge operations
#[derive(Debug, Error)]
pub enum ForgeError {
    #[error("No authentication token found")]
    NoToken,
    #[error("API error: {0}")]
    Api(String),
    #[error("Invalid repository format: {0}")]
    InvalidRepo(String),
    #[error("Pull request not found: {0}")]
    PrNotFound(u64),
    #[error("Not a supported forge URL: {0}")]
    UnsupportedUrl(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type ForgeResult<T> = Result<T, ForgeError>;

/// Information about a pull request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrInfo {
    pub number: u64,
    pub url: String,
    pub title: String,
    pub state: String,
    pub head_branch: String,
    pub base_branch: String,
    pub mergeable: Option<bool>,
    pub merged: bool,
}

/// Result of a merge operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeResult {
    pub merged: bool,
    pub sha: Option<String>,
    pub message: String,
}

/// Trait for source code forge providers (GitHub, GitLab, etc.)
#[async_trait]
pub trait ForgeProvider: Send + Sync {
    /// Create a pull request
    async fn create_pr(
        &self,
        repo: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> ForgeResult<PrInfo>;

    /// Get a pull request by number
    async fn get_pr(&self, repo: &str, number: u64) -> ForgeResult<PrInfo>;

    /// Find a PR by head branch
    async fn find_pr_by_branch(&self, repo: &str, head: &str) -> ForgeResult<Option<PrInfo>>;

    /// Merge a pull request
    async fn merge_pr(
        &self,
        repo: &str,
        number: u64,
        commit_title: Option<&str>,
    ) -> ForgeResult<MergeResult>;

    /// Parse a remote URL and return the repo identifier (e.g., "owner/repo")
    /// Returns None if the URL is not for this forge
    fn parse_remote_url(&self, url: &str) -> Option<String>;

    /// Get the forge type name
    fn forge_type(&self) -> &'static str;
}

/// Create a forge provider for the given remote URL
///
/// Currently only supports GitHub URLs.
pub fn create_forge_for_remote(remote_url: &str) -> ForgeResult<Box<dyn ForgeProvider>> {
    // Try GitHub
    let github = GitHubForge::new()?;
    if github.parse_remote_url(remote_url).is_some() {
        return Ok(Box::new(github));
    }

    Err(ForgeError::UnsupportedUrl(remote_url.to_string()))
}
