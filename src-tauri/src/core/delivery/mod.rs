//! Delivery Service - publishes run changes to git branches/PRs
//!
//! This module handles the delivery workflow for runs:
//! - Merge state checking (dry-run merge to detect conflicts)
//! - Staleness checking (commits on target since branch-off)
//! - Three delivery tiers: push branch, create PR, auto-merge
//! - Conflict resolution assistance
//! - Manual delivery tracking

use serde::{Deserialize, Serialize};
use std::process::Command;
use tracing::{debug, info};

use crate::core::github::{GitHubClient, PrInfo};
use crate::core::state::{DeliveryStatus, MergeState};

/// Error type for delivery operations
#[derive(Debug, thiserror::Error)]
pub enum DeliveryError {
    #[error("Git error: {0}")]
    Git(String),
    #[error("GitHub error: {0}")]
    GitHub(#[from] crate::core::github::GitHubError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Run not found: {0}")]
    RunNotFound(String),
    #[error("Invalid state: {0}")]
    InvalidState(String),
    #[error("Merge conflicts detected")]
    MergeConflicts,
    #[error("No remote configured")]
    NoRemote,
    #[error("Not a GitHub repository")]
    NotGitHub,
}

pub type DeliveryResult<T> = Result<T, DeliveryError>;

/// Current delivery state of a run
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryState {
    pub status: DeliveryStatus,
    pub merge_state: MergeState,
    pub staleness_commits: u32,
    pub delivery_branch: Option<String>,
    pub pr_url: Option<String>,
    pub pr_number: Option<u64>,
    pub conflicting_files: Vec<String>,
}

impl Default for DeliveryState {
    fn default() -> Self {
        Self {
            status: DeliveryStatus::Pending,
            merge_state: MergeState::Unknown,
            staleness_commits: 0,
            delivery_branch: None,
            pr_url: None,
            pr_number: None,
            conflicting_files: vec![],
        }
    }
}

/// Result of a push operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushResult {
    pub branch: String,
    pub remote: String,
    pub url: Option<String>,
}

/// Service for delivering run changes
pub struct DeliveryService {
    work_dir: std::path::PathBuf,
}

impl DeliveryService {
    /// Create a new delivery service for a run's work directory
    pub fn new(work_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    /// Run a git command in the work directory
    fn git(&self, args: &[&str]) -> DeliveryResult<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.work_dir)
            .output()?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(DeliveryError::Git(stderr.to_string()))
        }
    }

    /// Run a git command, returning None on failure instead of error
    fn git_optional(&self, args: &[&str]) -> Option<String> {
        self.git(args).ok()
    }

    // ========== MERGE STATE ==========

    /// Check merge state by doing a dry-run merge
    pub fn check_merge_state(&self, target_branch: &str) -> DeliveryResult<MergeState> {
        // Get current branch (reserved for future logging/debugging)
        let _current = self.git(&["rev-parse", "--abbrev-ref", "HEAD"])?;

        // Fetch latest from remote
        let _ = self.git(&["fetch", "origin", target_branch]);

        // Try a dry-run merge
        let merge_result = self.git(&[
            "merge-tree",
            "--write-tree",
            &format!("origin/{}", target_branch),
            "HEAD",
        ]);

        match merge_result {
            Ok(_) => {
                debug!("Merge to {} would be clean", target_branch);
                Ok(MergeState::Clean)
            }
            Err(DeliveryError::Git(msg)) if msg.contains("CONFLICT") => {
                debug!("Merge to {} has conflicts", target_branch);
                Ok(MergeState::Conflicts)
            }
            Err(_) => {
                // Fall back to checking with merge --no-commit
                self.check_merge_state_fallback(target_branch)
            }
        }
    }

    /// Fallback merge check using git merge --no-commit
    fn check_merge_state_fallback(&self, target_branch: &str) -> DeliveryResult<MergeState> {
        // Stash any changes first
        let _ = self.git(&["stash", "push", "-m", "delivery-check"]);

        // Try the merge
        let merge_result = self.git(&[
            "merge",
            "--no-commit",
            "--no-ff",
            &format!("origin/{}", target_branch),
        ]);

        // Abort the merge
        let _ = self.git(&["merge", "--abort"]);

        // Restore stash
        let _ = self.git(&["stash", "pop"]);

        match merge_result {
            Ok(_) => Ok(MergeState::Clean),
            Err(DeliveryError::Git(msg)) if msg.contains("CONFLICT") => Ok(MergeState::Conflicts),
            Err(_) => Ok(MergeState::Unknown),
        }
    }

    /// Get list of files that would conflict
    pub fn get_conflicting_files(&self, target_branch: &str) -> DeliveryResult<Vec<String>> {
        let _ = self.git(&["fetch", "origin", target_branch]);

        // Use merge-tree to find conflicts
        let output = self.git(&[
            "merge-tree",
            "--write-tree",
            "--name-only",
            &format!("origin/{}", target_branch),
            "HEAD",
        ]);

        match output {
            Ok(text) => {
                // Parse conflicting files from output
                let files: Vec<String> = text
                    .lines()
                    .filter(|line| !line.is_empty() && !line.starts_with("Auto-merging"))
                    .map(|s| s.to_string())
                    .collect();
                Ok(files)
            }
            Err(_) => Ok(vec![]),
        }
    }

    /// Count commits on target branch since branch-off point
    pub fn check_staleness(
        &self,
        target_branch: &str,
        branch_off_commit: &str,
    ) -> DeliveryResult<u32> {
        let _ = self.git(&["fetch", "origin", target_branch]);

        let count = self.git(&[
            "rev-list",
            "--count",
            &format!("{}..origin/{}", branch_off_commit, target_branch),
        ])?;

        Ok(count.parse().unwrap_or(0))
    }

    /// Get current branch name
    pub fn current_branch(&self) -> DeliveryResult<String> {
        self.git(&["rev-parse", "--abbrev-ref", "HEAD"])
    }

    /// Get remote URL
    pub fn remote_url(&self) -> DeliveryResult<String> {
        self.git(&["remote", "get-url", "origin"])
    }

    /// Get GitHub repo identifier (owner/repo) from remote URL
    pub fn github_repo(&self) -> DeliveryResult<String> {
        let url = self.remote_url()?;
        GitHubClient::parse_remote_url(&url).ok_or(DeliveryError::NotGitHub)
    }

    // ========== DELIVERY TIERS ==========

    /// Tier 1: Push branch to remote
    pub fn push_branch(&self, branch: Option<&str>) -> DeliveryResult<PushResult> {
        let branch = match branch {
            Some(b) => b.to_string(),
            None => self.current_branch()?,
        };

        info!("Pushing branch {} to origin", branch);

        // Push with upstream tracking
        self.git(&["push", "-u", "origin", &branch])?;

        let remote_url = self.remote_url()?;
        let url = self.make_branch_url(&remote_url, &branch);

        Ok(PushResult {
            branch,
            remote: "origin".to_string(),
            url,
        })
    }

    /// Tier 2: Push branch and create PR
    pub async fn create_pr(
        &self,
        target_branch: &str,
        title: &str,
        body: &str,
    ) -> DeliveryResult<PrInfo> {
        // First push the branch
        let push_result = self.push_branch(None)?;

        // Get repo info
        let repo = self.github_repo()?;

        // Create PR via GitHub API
        let client = GitHubClient::new()?;
        let pr = client
            .create_pr(&repo, &push_result.branch, target_branch, title, body)
            .await?;

        info!("Created PR #{}: {}", pr.number, pr.url);

        Ok(pr)
    }

    /// Tier 3: Push, create PR, and auto-merge (only if clean)
    pub async fn auto_merge(
        &self,
        target_branch: &str,
        title: &str,
        body: &str,
    ) -> DeliveryResult<crate::core::github::MergeInfo> {
        // Check merge state first
        let merge_state = self.check_merge_state(target_branch)?;
        if merge_state == MergeState::Conflicts {
            return Err(DeliveryError::MergeConflicts);
        }

        // Create the PR
        let pr = self.create_pr(target_branch, title, body).await?;

        // Merge the PR
        let repo = self.github_repo()?;
        let client = GitHubClient::new()?;
        let merge_result = client.merge_pr(&repo, pr.number, Some(title)).await?;

        info!("Merged PR #{}", pr.number);

        Ok(merge_result)
    }

    // ========== DELIVERY STATE ==========

    /// Get full delivery state for a run
    pub fn get_delivery_state(
        &self,
        target_branch: &str,
        branch_off_commit: Option<&str>,
    ) -> DeliveryResult<DeliveryState> {
        let mut state = DeliveryState::default();

        // Get current branch as delivery branch
        state.delivery_branch = self.current_branch().ok();

        // Check merge state
        state.merge_state = self
            .check_merge_state(target_branch)
            .unwrap_or(MergeState::Unknown);

        // Get conflicting files if there are conflicts
        if state.merge_state == MergeState::Conflicts {
            state.conflicting_files = self
                .get_conflicting_files(target_branch)
                .unwrap_or_default();
        }

        // Check staleness
        if let Some(commit) = branch_off_commit {
            state.staleness_commits = self.check_staleness(target_branch, commit).unwrap_or(0);
        }

        // Check if there's an existing PR
        if let Some(ref branch) = state.delivery_branch {
            if let Ok(repo) = self.github_repo() {
                let rt = tokio::runtime::Handle::try_current();
                if let Ok(handle) = rt {
                    if let Ok(client) = GitHubClient::new() {
                        let branch_clone = branch.clone();
                        let repo_clone = repo.clone();
                        if let Ok(Some(pr)) = handle.block_on(async {
                            client.find_pr_by_branch(&repo_clone, &branch_clone).await
                        }) {
                            state.pr_url = Some(pr.url);
                            state.pr_number = Some(pr.number);
                            state.status = if pr.merged {
                                DeliveryStatus::Merged
                            } else {
                                DeliveryStatus::PrOpen
                            };
                        }
                    }
                }
            }
        }

        Ok(state)
    }

    /// Get delivery state async (preferred)
    pub async fn get_delivery_state_async(
        &self,
        target_branch: &str,
        branch_off_commit: Option<&str>,
    ) -> DeliveryResult<DeliveryState> {
        let mut state = DeliveryState::default();

        // Get current branch as delivery branch
        state.delivery_branch = self.current_branch().ok();

        // Check merge state
        state.merge_state = self
            .check_merge_state(target_branch)
            .unwrap_or(MergeState::Unknown);

        // Get conflicting files if there are conflicts
        if state.merge_state == MergeState::Conflicts {
            state.conflicting_files = self
                .get_conflicting_files(target_branch)
                .unwrap_or_default();
        }

        // Check staleness
        if let Some(commit) = branch_off_commit {
            state.staleness_commits = self.check_staleness(target_branch, commit).unwrap_or(0);
        }

        // Check if there's an existing PR
        if let Some(ref branch) = state.delivery_branch {
            if let Ok(repo) = self.github_repo() {
                if let Ok(client) = GitHubClient::new() {
                    if let Ok(Some(pr)) = client.find_pr_by_branch(&repo, branch).await {
                        state.pr_url = Some(pr.url);
                        state.pr_number = Some(pr.number);
                        state.status = if pr.merged {
                            DeliveryStatus::Merged
                        } else {
                            DeliveryStatus::PrOpen
                        };
                    }
                }
            }
        }

        Ok(state)
    }

    // ========== HELPERS ==========

    /// Make a URL to view a branch on GitHub
    fn make_branch_url(&self, remote_url: &str, branch: &str) -> Option<String> {
        let repo = GitHubClient::parse_remote_url(remote_url)?;
        Some(format!("https://github.com/{}/tree/{}", repo, branch))
    }

    /// Generate a delivery branch name from run name
    pub fn delivery_branch_name(run_name: &str) -> String {
        format!("hirsel/{}", run_name)
    }

    /// Generate a PR title from run name
    pub fn pr_title(run_name: &str, summary: Option<&str>) -> String {
        match summary {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => format!("Changes from {}", run_name),
        }
    }

    /// Generate a PR body
    pub fn pr_body(run_name: &str, task_ids: &[String], eval_ids: &[String]) -> String {
        let mut body = String::new();

        body.push_str(&format!("## Run: {}\n\n", run_name));

        if !task_ids.is_empty() {
            body.push_str("### Tasks\n");
            for task_id in task_ids {
                body.push_str(&format!("- {}\n", task_id));
            }
            body.push('\n');
        }

        if !eval_ids.is_empty() {
            body.push_str("### Evaluations\n");
            for eval_id in eval_ids {
                body.push_str(&format!("- {}\n", eval_id));
            }
            body.push('\n');
        }

        body.push_str("---\n");
        body.push_str("*Generated by [Hirsel](https://github.com/anthropics/hirsel)*\n");

        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delivery_branch_name() {
        assert_eq!(
            DeliveryService::delivery_branch_name("jolly-parrot"),
            "hirsel/jolly-parrot"
        );
    }

    #[test]
    fn test_pr_title() {
        assert_eq!(
            DeliveryService::pr_title("jolly-parrot", Some("Add user auth")),
            "Add user auth"
        );
        assert_eq!(
            DeliveryService::pr_title("jolly-parrot", None),
            "Changes from jolly-parrot"
        );
        assert_eq!(
            DeliveryService::pr_title("jolly-parrot", Some("")),
            "Changes from jolly-parrot"
        );
    }

    #[test]
    fn test_pr_body() {
        let body = DeliveryService::pr_body(
            "jolly-parrot",
            &["build-api".to_string(), "user-endpoints".to_string()],
            &["api-test".to_string()],
        );

        assert!(body.contains("## Run: jolly-parrot"));
        assert!(body.contains("### Tasks"));
        assert!(body.contains("- build-api"));
        assert!(body.contains("### Evaluations"));
        assert!(body.contains("- api-test"));
    }
}
