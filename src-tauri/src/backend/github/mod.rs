//! GitHub API client for Hirsel
//!
//! Provides authenticated GitHub API access for creating PRs, checking merge status, etc.
//! Auth is resolved in priority order:
//! 1. GITHUB_TOKEN environment variable
//! 2. gh CLI config (~/.config/gh/hosts.yml)
//! 3. Hirsel config (~/.hirsel/config.toml)

use octocrab::models::pulls::PullRequest;
use octocrab::params::pulls::Sort;
use octocrab::Octocrab;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, info};

/// Error type for GitHub operations
#[derive(Debug, thiserror::Error)]
pub enum GitHubError {
    #[error("No GitHub token found. Set GITHUB_TOKEN env var, run 'gh auth login', or add github_token to hirsel config")]
    NoToken,
    #[error("GitHub API error: {0}")]
    Api(#[from] octocrab::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("Invalid repository format: {0}")]
    InvalidRepo(String),
    #[error("Pull request not found: {0}")]
    PrNotFound(u64),
}

pub type GitHubResult<T> = Result<T, GitHubError>;

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

impl From<PullRequest> for PrInfo {
    fn from(pr: PullRequest) -> Self {
        Self {
            number: pr.number,
            url: pr.html_url.map(|u| u.to_string()).unwrap_or_default(),
            title: pr.title.unwrap_or_default(),
            state: pr.state.map(|s| format!("{:?}", s)).unwrap_or_default(),
            head_branch: pr.head.ref_field,
            base_branch: pr.base.ref_field,
            mergeable: pr.mergeable,
            merged: pr.merged.unwrap_or(false),
        }
    }
}

/// Result of a merge operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeInfo {
    pub merged: bool,
    pub sha: Option<String>,
    pub message: String,
}

/// GitHub API client
pub struct GitHubClient {
    client: Octocrab,
}

impl GitHubClient {
    /// Create a new GitHub client, resolving auth from available sources
    pub fn new() -> GitHubResult<Self> {
        let token = Self::resolve_token()?;
        let client = Octocrab::builder()
            .personal_token(token)
            .build()
            .map_err(GitHubError::Api)?;

        Ok(Self { client })
    }

    /// Create a GitHub client with a specific token
    pub fn with_token(token: String) -> GitHubResult<Self> {
        let client = Octocrab::builder()
            .personal_token(token)
            .build()
            .map_err(GitHubError::Api)?;

        Ok(Self { client })
    }

    /// Resolve GitHub token from available sources
    fn resolve_token() -> GitHubResult<String> {
        // 1. Check GITHUB_TOKEN env var
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if !token.is_empty() {
                debug!("Using GITHUB_TOKEN from environment");
                return Ok(token);
            }
        }

        // 2. Check gh CLI config
        if let Some(token) = Self::read_gh_config()? {
            debug!("Using token from gh CLI config");
            return Ok(token);
        }

        // 3. Check hirsel config
        if let Some(token) = Self::read_hirsel_config()? {
            debug!("Using token from hirsel config");
            return Ok(token);
        }

        Err(GitHubError::NoToken)
    }

    /// Read token from gh CLI config (~/.config/gh/hosts.yml)
    fn read_gh_config() -> GitHubResult<Option<String>> {
        let config_path = Self::gh_config_path();
        if !config_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&config_path)?;

        // Parse the YAML structure: hosts.github.com.oauth_token
        #[derive(Deserialize)]
        struct GhHosts {
            #[serde(rename = "github.com")]
            github_com: Option<GhHost>,
        }

        #[derive(Deserialize)]
        struct GhHost {
            oauth_token: Option<String>,
        }

        let hosts: GhHosts = serde_yaml::from_str(&content)?;
        Ok(hosts.github_com.and_then(|h| h.oauth_token))
    }

    /// Get the gh CLI config path
    fn gh_config_path() -> PathBuf {
        // XDG_CONFIG_HOME or ~/.config
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".config")
            });

        config_dir.join("gh").join("hosts.yml")
    }

    /// Read token from hirsel config (~/.hirsel/config.toml)
    fn read_hirsel_config() -> GitHubResult<Option<String>> {
        let config_path = crate::backend::config::hirsel_dir().join("config.toml");
        if !config_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&config_path)?;

        // Parse TOML looking for github_token
        #[derive(Deserialize)]
        struct HirselConfig {
            github_token: Option<String>,
        }

        let config: HirselConfig =
            toml::from_str(&content).map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(config.github_token)
    }

    /// Parse owner/repo from a repository string
    fn parse_repo(repo: &str) -> GitHubResult<(&str, &str)> {
        let parts: Vec<&str> = repo.split('/').collect();
        if parts.len() != 2 {
            return Err(GitHubError::InvalidRepo(format!(
                "Expected 'owner/repo' format, got: {}",
                repo
            )));
        }
        Ok((parts[0], parts[1]))
    }

    /// Create a pull request
    pub async fn create_pr(
        &self,
        repo: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> GitHubResult<PrInfo> {
        let (owner, repo_name) = Self::parse_repo(repo)?;

        info!(
            "Creating PR in {}/{}: {} -> {}",
            owner, repo_name, head, base
        );

        let pr = self
            .client
            .pulls(owner, repo_name)
            .create(title, head, base)
            .body(body)
            .send()
            .await?;

        Ok(pr.into())
    }

    /// Get a pull request by number
    pub async fn get_pr(&self, repo: &str, number: u64) -> GitHubResult<PrInfo> {
        let (owner, repo_name) = Self::parse_repo(repo)?;

        let pr = self.client.pulls(owner, repo_name).get(number).await?;

        Ok(pr.into())
    }

    /// Find a PR by head branch
    pub async fn find_pr_by_branch(&self, repo: &str, head: &str) -> GitHubResult<Option<PrInfo>> {
        let (owner, repo_name) = Self::parse_repo(repo)?;

        // List open PRs and filter by head branch
        let prs = self
            .client
            .pulls(owner, repo_name)
            .list()
            .state(octocrab::params::State::Open)
            .head(format!("{}:{}", owner, head))
            .sort(Sort::Created)
            .send()
            .await?;

        Ok(prs.items.into_iter().next().map(|pr| pr.into()))
    }

    /// Merge a pull request
    pub async fn merge_pr(
        &self,
        repo: &str,
        number: u64,
        commit_title: Option<&str>,
    ) -> GitHubResult<MergeInfo> {
        let (owner, repo_name) = Self::parse_repo(repo)?;

        info!("Merging PR #{} in {}/{}", number, owner, repo_name);

        // Check if PR is mergeable first
        let pr = self.get_pr(repo, number).await?;
        if pr.merged {
            return Ok(MergeInfo {
                merged: true,
                sha: None,
                message: "PR was already merged".to_string(),
            });
        }

        // Keep the pulls handler alive for the merge builder
        let pulls = self.client.pulls(owner, repo_name);
        let mut merge = pulls.merge(number);
        if let Some(title) = commit_title {
            merge = merge.title(title);
        }

        let result = merge.send().await?;

        Ok(MergeInfo {
            merged: result.merged,
            sha: result.sha,
            message: result.message.unwrap_or_default(),
        })
    }

    /// Check if a PR can be merged (no conflicts)
    pub async fn check_mergeable(&self, repo: &str, number: u64) -> GitHubResult<Option<bool>> {
        let pr = self.get_pr(repo, number).await?;
        Ok(pr.mergeable)
    }

    /// Get repository info (owner, name) from a git remote URL
    pub fn parse_remote_url(url: &str) -> Option<String> {
        // Handle various URL formats:
        // https://github.com/owner/repo.git
        // git@github.com:owner/repo.git
        // ssh://git@github.com/owner/repo.git

        let url = url.trim();

        // SSH format: git@github.com:owner/repo.git
        if url.starts_with("git@github.com:") {
            let path = url.strip_prefix("git@github.com:")?.strip_suffix(".git")?;
            return Some(path.to_string());
        }

        // HTTPS format: https://github.com/owner/repo.git
        if let Some(path) = url.strip_prefix("https://github.com/") {
            let path = path.strip_suffix(".git").unwrap_or(path);
            return Some(path.to_string());
        }

        // SSH format: ssh://git@github.com/owner/repo.git
        if let Some(path) = url.strip_prefix("ssh://git@github.com/") {
            let path = path.strip_suffix(".git").unwrap_or(path);
            return Some(path.to_string());
        }

        None
    }

    /// Get the current user's login
    pub async fn get_current_user(&self) -> GitHubResult<String> {
        let user = self.client.current().user().await?;
        Ok(user.login)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_remote_url() {
        assert_eq!(
            GitHubClient::parse_remote_url("https://github.com/owner/repo.git"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            GitHubClient::parse_remote_url("https://github.com/owner/repo"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            GitHubClient::parse_remote_url("git@github.com:owner/repo.git"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            GitHubClient::parse_remote_url("ssh://git@github.com/owner/repo.git"),
            Some("owner/repo".to_string())
        );
        assert_eq!(GitHubClient::parse_remote_url("not-a-github-url"), None);
    }

    #[test]
    fn test_parse_repo() {
        let (owner, repo) = GitHubClient::parse_repo("owner/repo").unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");

        assert!(GitHubClient::parse_repo("invalid").is_err());
        assert!(GitHubClient::parse_repo("too/many/parts").is_err());
    }
}
