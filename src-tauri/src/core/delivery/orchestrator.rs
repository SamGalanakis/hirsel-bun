//! Delivery orchestrator - unified entry point for delivery operations
//!
//! Combines workspace resolution, git operations, and forge operations
//! into a single cohesive API.

use serde::{Deserialize, Serialize};
use tracing::info;

use super::git_ops::{GitOperations, PushResult};
use super::workspace::{resolve_workspace_for_project, WorkspaceLocation};
use super::{DeliveryError, DeliveryResult};
use crate::core::forge::{create_forge_for_remote, ForgeProvider, MergeResult, PrInfo};
use crate::core::state::MergeState;

/// Current delivery state
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

/// Delivery status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Pushed,
    PrOpen,
    Merged,
    Abandoned,
}

/// Delivery orchestrator for a project
pub struct DeliveryOrchestrator {
    workspace: WorkspaceLocation,
    git_ops: Option<GitOperations>,
    forge: Option<Box<dyn ForgeProvider>>,
    repo: Option<String>,
}

impl DeliveryOrchestrator {
    /// Create a delivery orchestrator for a project
    pub async fn for_project(project_id: i64) -> DeliveryResult<Self> {
        let workspace = resolve_workspace_for_project(project_id)
            .await
            .map_err(|e| DeliveryError::InvalidState(e.to_string()))?;

        let (git_ops, forge, repo) = match &workspace {
            WorkspaceLocation::Local(path) => {
                let ops = GitOperations::new(path);

                // Get remote URL and create forge
                let remote_url = ops.remote_url().ok();
                let (forge, repo) = if let Some(ref url) = remote_url {
                    let forge = create_forge_for_remote(url).ok();
                    let repo = forge.as_ref().and_then(|f| f.parse_remote_url(url));
                    (forge, repo)
                } else {
                    (None, None)
                };

                (Some(ops), forge, repo)
            }
            WorkspaceLocation::Coordinator { .. } => {
                // For coordinator mode, we'd need to proxy through the coordinator
                // Not yet implemented
                (None, None, None)
            }
        };

        Ok(Self {
            workspace,
            git_ops,
            forge,
            repo,
        })
    }

    /// Create a delivery orchestrator from a work directory path
    pub fn from_work_dir(work_dir: impl Into<std::path::PathBuf>) -> DeliveryResult<Self> {
        let path = work_dir.into();
        let ops = GitOperations::new(&path);

        let remote_url = ops.remote_url().ok();
        let (forge, repo) = if let Some(ref url) = remote_url {
            let forge = create_forge_for_remote(url).ok();
            let repo = forge.as_ref().and_then(|f| f.parse_remote_url(url));
            (forge, repo)
        } else {
            (None, None)
        };

        Ok(Self {
            workspace: WorkspaceLocation::Local(path),
            git_ops: Some(ops),
            forge,
            repo,
        })
    }

    /// Get the workspace location
    pub fn workspace(&self) -> &WorkspaceLocation {
        &self.workspace
    }

    /// Check if this orchestrator has a forge (can create PRs)
    pub fn has_forge(&self) -> bool {
        self.forge.is_some()
    }

    /// Get the git operations (if local workspace)
    pub fn git_ops(&self) -> Option<&GitOperations> {
        self.git_ops.as_ref()
    }

    // ========== Tier 1: Push Branch ==========

    /// Push branch to remote
    pub fn push_branch(&self, branch: Option<&str>) -> DeliveryResult<PushResult> {
        let git = self
            .git_ops
            .as_ref()
            .ok_or_else(|| DeliveryError::InvalidState("No local workspace".to_string()))?;

        git.push_branch(branch)
    }

    // ========== Tier 2: Create PR ==========

    /// Push branch and create PR
    pub async fn create_pr(
        &self,
        target_branch: &str,
        title: &str,
        body: &str,
    ) -> DeliveryResult<PrInfo> {
        // First push the branch
        let push_result = self.push_branch(None)?;

        // Get forge and repo
        let forge = self
            .forge
            .as_ref()
            .ok_or_else(|| DeliveryError::NotGitHub)?;
        let repo = self.repo.as_ref().ok_or_else(|| DeliveryError::NotGitHub)?;

        // Create PR
        let pr = forge
            .create_pr(repo, &push_result.branch, target_branch, title, body)
            .await
            .map_err(|e| DeliveryError::InvalidState(e.to_string()))?;

        info!("Created PR #{}: {}", pr.number, pr.url);

        Ok(pr)
    }

    // ========== Tier 3: Auto-merge ==========

    /// Push, create PR, and auto-merge (only if clean)
    pub async fn auto_merge(
        &self,
        target_branch: &str,
        title: &str,
        body: &str,
    ) -> DeliveryResult<MergeResult> {
        // Check merge state first
        let merge_state = self.check_merge_state(target_branch)?;
        if merge_state == MergeState::Conflicts {
            return Err(DeliveryError::MergeConflicts);
        }

        // Create the PR
        let pr = self.create_pr(target_branch, title, body).await?;

        // Merge the PR
        let forge = self.forge.as_ref().ok_or(DeliveryError::NotGitHub)?;
        let repo = self.repo.as_ref().ok_or(DeliveryError::NotGitHub)?;

        let merge_result = forge
            .merge_pr(repo, pr.number, Some(title))
            .await
            .map_err(|e| DeliveryError::InvalidState(e.to_string()))?;

        info!("Merged PR #{}", pr.number);

        Ok(merge_result)
    }

    // ========== State Queries ==========

    /// Check merge state against target branch
    pub fn check_merge_state(&self, target_branch: &str) -> DeliveryResult<MergeState> {
        let git = self
            .git_ops
            .as_ref()
            .ok_or_else(|| DeliveryError::InvalidState("No local workspace".to_string()))?;

        git.check_merge_state(target_branch)
    }

    /// Get conflicting files
    pub fn get_conflicting_files(&self, target_branch: &str) -> DeliveryResult<Vec<String>> {
        let git = self
            .git_ops
            .as_ref()
            .ok_or_else(|| DeliveryError::InvalidState("No local workspace".to_string()))?;

        git.get_conflicting_files(target_branch)
    }

    /// Check staleness (commits on target since branch-off)
    pub fn check_staleness(
        &self,
        target_branch: &str,
        branch_off_commit: &str,
    ) -> DeliveryResult<u32> {
        let git = self
            .git_ops
            .as_ref()
            .ok_or_else(|| DeliveryError::InvalidState("No local workspace".to_string()))?;

        git.check_staleness(target_branch, branch_off_commit)
    }

    /// Get current branch
    pub fn current_branch(&self) -> DeliveryResult<String> {
        let git = self
            .git_ops
            .as_ref()
            .ok_or_else(|| DeliveryError::InvalidState("No local workspace".to_string()))?;

        git.current_branch()
    }

    /// Get full delivery state
    pub async fn get_delivery_state(
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
        if let (Some(forge), Some(repo), Some(ref branch)) =
            (&self.forge, &self.repo, &state.delivery_branch)
        {
            if let Ok(Some(pr)) = forge.find_pr_by_branch(repo, branch).await {
                state.pr_url = Some(pr.url);
                state.pr_number = Some(pr.number);
                state.status = if pr.merged {
                    DeliveryStatus::Merged
                } else {
                    DeliveryStatus::PrOpen
                };
            }
        }

        Ok(state)
    }

    // ========== Conflict Resolution ==========

    /// Start a merge that may have conflicts
    pub fn start_merge_with_conflicts(&self, target_branch: &str) -> DeliveryResult<Vec<String>> {
        let git = self
            .git_ops
            .as_ref()
            .ok_or_else(|| DeliveryError::InvalidState("No local workspace".to_string()))?;

        git.start_merge_with_conflicts(target_branch)
    }

    /// Complete merge after conflicts are resolved
    pub fn complete_merge(&self, commit_message: &str) -> DeliveryResult<String> {
        let git = self
            .git_ops
            .as_ref()
            .ok_or_else(|| DeliveryError::InvalidState("No local workspace".to_string()))?;

        git.complete_merge(commit_message)
    }

    /// Abort an in-progress merge
    pub fn abort_merge(&self) -> DeliveryResult<()> {
        let git = self
            .git_ops
            .as_ref()
            .ok_or_else(|| DeliveryError::InvalidState("No local workspace".to_string()))?;

        git.abort_merge()
    }
}
