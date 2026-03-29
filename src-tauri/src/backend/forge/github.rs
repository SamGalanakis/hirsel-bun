//! GitHub forge implementation

use async_trait::async_trait;
use octocrab::models::pulls::PullRequest;
use octocrab::params::pulls::Sort;
use octocrab::Octocrab;
use std::path::PathBuf;
use tracing::{debug, info};

use super::{ForgeError, ForgeProvider, ForgeResult, MergeResult, PrInfo};

impl From<octocrab::Error> for ForgeError {
    fn from(e: octocrab::Error) -> Self {
        ForgeError::Api(e.to_string())
    }
}

impl From<serde_yaml::Error> for ForgeError {
    fn from(e: serde_yaml::Error) -> Self {
        ForgeError::Api(format!("YAML parse error: {}", e))
    }
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

/// GitHub forge provider
pub struct GitHubForge {
    client: Octocrab,
}

impl GitHubForge {
    /// Create a new GitHub forge, resolving auth from available sources
    pub fn new() -> ForgeResult<Self> {
        let token = Self::resolve_token()?;
        let client = Octocrab::builder()
            .personal_token(token)
            .build()
            .map_err(ForgeError::from)?;

        Ok(Self { client })
    }

    /// Create a GitHub forge with a specific token
    pub fn with_token(token: String) -> ForgeResult<Self> {
        let client = Octocrab::builder()
            .personal_token(token)
            .build()
            .map_err(ForgeError::from)?;

        Ok(Self { client })
    }

    /// Resolve GitHub token from available sources:
    /// 1. GITHUB_TOKEN environment variable
    /// 2. gh CLI config (~/.config/gh/hosts.yml)
    /// 3. Hirsel config (~/.hirsel/config.toml)
    fn resolve_token() -> ForgeResult<String> {
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

        Err(ForgeError::NoToken)
    }

    /// Read token from gh CLI config (~/.config/gh/hosts.yml)
    fn read_gh_config() -> ForgeResult<Option<String>> {
        let config_path = Self::gh_config_path();
        if !config_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&config_path)?;

        #[derive(serde::Deserialize)]
        struct GhHosts {
            #[serde(rename = "github.com")]
            github_com: Option<GhHost>,
        }

        #[derive(serde::Deserialize)]
        struct GhHost {
            oauth_token: Option<String>,
        }

        let hosts: GhHosts = serde_yaml::from_str(&content)?;
        Ok(hosts.github_com.and_then(|h| h.oauth_token))
    }

    /// Get the gh CLI config path
    fn gh_config_path() -> PathBuf {
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
    fn read_hirsel_config() -> ForgeResult<Option<String>> {
        let config_path = crate::backend::config::hirsel_dir().join("config.toml");
        if !config_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&config_path)?;

        #[derive(serde::Deserialize)]
        struct HirselConfig {
            github_token: Option<String>,
        }

        let config: HirselConfig =
            toml::from_str(&content).map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(config.github_token)
    }

    /// Parse owner/repo from a repository string
    fn parse_repo(repo: &str) -> ForgeResult<(&str, &str)> {
        let parts: Vec<&str> = repo.split('/').collect();
        if parts.len() != 2 {
            return Err(ForgeError::InvalidRepo(format!(
                "Expected 'owner/repo' format, got: {}",
                repo
            )));
        }
        Ok((parts[0], parts[1]))
    }
}

#[async_trait]
impl ForgeProvider for GitHubForge {
    async fn create_pr(
        &self,
        repo: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> ForgeResult<PrInfo> {
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

    async fn get_pr(&self, repo: &str, number: u64) -> ForgeResult<PrInfo> {
        let (owner, repo_name) = Self::parse_repo(repo)?;
        let pr = self.client.pulls(owner, repo_name).get(number).await?;
        Ok(pr.into())
    }

    async fn find_pr_by_branch(&self, repo: &str, head: &str) -> ForgeResult<Option<PrInfo>> {
        let (owner, repo_name) = Self::parse_repo(repo)?;

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

    async fn merge_pr(
        &self,
        repo: &str,
        number: u64,
        commit_title: Option<&str>,
    ) -> ForgeResult<MergeResult> {
        let (owner, repo_name) = Self::parse_repo(repo)?;

        info!("Merging PR #{} in {}/{}", number, owner, repo_name);

        // Check if PR is mergeable first
        let pr = self.get_pr(repo, number).await?;
        if pr.merged {
            return Ok(MergeResult {
                merged: true,
                sha: None,
                message: "PR was already merged".to_string(),
            });
        }

        let pulls = self.client.pulls(owner, repo_name);
        let mut merge = pulls.merge(number);
        if let Some(title) = commit_title {
            merge = merge.title(title);
        }

        let result = merge.send().await?;

        Ok(MergeResult {
            merged: result.merged,
            sha: result.sha,
            message: result.message.unwrap_or_default(),
        })
    }

    fn parse_remote_url(&self, url: &str) -> Option<String> {
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

    fn forge_type(&self) -> &'static str {
        "github"
    }
}

/// Parse a GitHub remote URL and return the repo identifier (owner/repo)
/// Standalone function for use without creating a GitHubForge instance
pub fn parse_github_remote_url(url: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_remote_url() {
        // Test the static parsing function
        assert_eq!(
            parse_github_remote_url("https://github.com/owner/repo.git"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            parse_github_remote_url("https://github.com/owner/repo"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            parse_github_remote_url("git@github.com:owner/repo.git"),
            Some("owner/repo".to_string())
        );
        assert_eq!(
            parse_github_remote_url("ssh://git@github.com/owner/repo.git"),
            Some("owner/repo".to_string())
        );
        assert_eq!(parse_github_remote_url("not-a-github-url"), None);
    }
}
