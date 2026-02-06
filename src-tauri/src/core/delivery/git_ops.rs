//! Git operations for delivery
//!
//! Extracted git command execution for push, fetch, merge state checking, etc.

use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::{debug, info};

use super::{DeliveryError, DeliveryResult};
use crate::core::state::MergeState;

/// Result of a push operation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushResult {
    pub branch: String,
    pub remote: String,
    pub url: Option<String>,
}

/// Git operations for a workspace
pub struct GitOperations {
    work_dir: PathBuf,
}

impl GitOperations {
    /// Create git operations for a work directory
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    /// Get the work directory path
    pub fn work_dir(&self) -> &Path {
        &self.work_dir
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

    /// Get current branch name
    pub fn current_branch(&self) -> DeliveryResult<String> {
        self.git(&["rev-parse", "--abbrev-ref", "HEAD"])
    }

    /// Get remote URL
    pub fn remote_url(&self) -> DeliveryResult<String> {
        self.git(&["remote", "get-url", "origin"])
    }

    /// Ensure the origin remote is configured with the given URL.
    ///
    /// Adds origin if missing, updates it if it differs from the expected URL.
    pub fn ensure_remote(&self, url: &str) -> DeliveryResult<()> {
        match self.remote_url() {
            Ok(current) if current == url => Ok(()),
            Ok(_) => {
                info!("Updating origin remote to {}", url);
                self.git(&["remote", "set-url", "origin", url])?;
                Ok(())
            }
            Err(_) => {
                info!("Adding origin remote: {}", url);
                self.git(&["remote", "add", "origin", url])?;
                Ok(())
            }
        }
    }

    /// Check if a ref exists (rev-parse --verify)
    pub fn rev_parse(&self, refspec: &str) -> DeliveryResult<String> {
        self.git(&["rev-parse", "--verify", refspec])
    }

    /// Fetch from remote
    pub fn fetch(&self, branch: &str) -> DeliveryResult<()> {
        let _ = self.git(&["fetch", "origin", branch]);
        Ok(())
    }

    /// Push branch to remote with upstream tracking.
    ///
    /// If `delivery_branch` is provided, pushes `HEAD` to that branch name on
    /// the remote (`git push -u origin HEAD:<delivery_branch>`).  Otherwise
    /// pushes the current branch by its own name.
    pub fn push_branch(
        &self,
        branch: Option<&str>,
        delivery_branch: Option<&str>,
    ) -> DeliveryResult<PushResult> {
        let remote_branch = match delivery_branch.or(branch) {
            Some(b) => b.to_string(),
            None => self.current_branch()?,
        };

        info!("Pushing HEAD to origin as {}", remote_branch);

        let refspec = format!("HEAD:{}", remote_branch);
        self.git(&["push", "-u", "origin", &refspec])?;

        let remote_url = self.remote_url()?;
        let url = self.make_branch_url(&remote_url, &remote_branch);

        Ok(PushResult {
            branch: remote_branch,
            remote: "origin".to_string(),
            url,
        })
    }

    /// Validate that a remote URL is reachable and list its branches.
    pub fn validate_remote(url: &str) -> DeliveryResult<Vec<String>> {
        let output = Command::new("git")
            .args(["ls-remote", "--heads", url])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DeliveryError::Git(format!(
                "Remote unreachable: {}",
                stderr.trim()
            )));
        }

        let branches = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split("refs/heads/").nth(1))
            .map(|s| s.to_string())
            .collect();

        Ok(branches)
    }

    /// Initialize a bare-ish git repo at the given path so it can receive pushes.
    pub fn init_repo(path: &Path) -> DeliveryResult<()> {
        let output = Command::new("git")
            .args(["init", "--bare"])
            .arg(path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DeliveryError::Git(format!(
                "Failed to init repo: {}",
                stderr.trim()
            )));
        }

        info!("Initialized bare repo at {}", path.display());
        Ok(())
    }

    /// Check merge state by doing a dry-run merge
    pub fn check_merge_state(&self, target_branch: &str) -> DeliveryResult<MergeState> {
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
            Err(_) => self.check_merge_state_fallback(target_branch),
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

        let output = self.git(&[
            "merge-tree",
            "--write-tree",
            "--name-only",
            &format!("origin/{}", target_branch),
            "HEAD",
        ]);

        match output {
            Ok(text) => {
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

    /// Start a merge that may have conflicts (returns conflicting files)
    pub fn start_merge_with_conflicts(&self, target_branch: &str) -> DeliveryResult<Vec<String>> {
        let _ = self.git(&["fetch", "origin", target_branch]);

        let merge_result = self.git(&[
            "merge",
            "--no-commit",
            "--no-ff",
            &format!("origin/{}", target_branch),
        ]);

        match merge_result {
            Ok(_) => {
                debug!("Merge with {} is clean", target_branch);
                Ok(vec![])
            }
            Err(DeliveryError::Git(msg)) if msg.contains("CONFLICT") => {
                let conflicts = self.get_working_tree_conflicts()?;
                info!(
                    "Started merge with {} - {} conflicting files",
                    target_branch,
                    conflicts.len()
                );
                Ok(conflicts)
            }
            Err(e) => Err(e),
        }
    }

    /// Get list of files with conflict markers in the working tree
    pub fn get_working_tree_conflicts(&self) -> DeliveryResult<Vec<String>> {
        let output = self.git(&["status", "--porcelain"])?;

        let conflicts: Vec<String> = output
            .lines()
            .filter(|line| {
                line.starts_with("UU ")
                    || line.starts_with("AA ")
                    || line.starts_with("DD ")
                    || line.starts_with("AU ")
                    || line.starts_with("UA ")
                    || line.starts_with("DU ")
                    || line.starts_with("UD ")
            })
            .map(|line| line[3..].to_string())
            .collect();

        Ok(conflicts)
    }

    /// Verify no conflict markers remain in the working tree
    pub fn verify_no_conflict_markers(&self) -> DeliveryResult<()> {
        let result = Command::new("grep")
            .args(["-r", "-l", "<<<<<<<", "."])
            .current_dir(&self.work_dir)
            .output()?;

        if result.status.success() {
            let files: Vec<String> = String::from_utf8_lossy(&result.stdout)
                .lines()
                .map(|s| s.to_string())
                .collect();

            if !files.is_empty() {
                return Err(DeliveryError::ConflictMarkersRemain(files));
            }
        }

        Ok(())
    }

    /// Complete the merge after conflicts have been resolved
    pub fn complete_merge(&self, commit_message: &str) -> DeliveryResult<String> {
        self.verify_no_conflict_markers()?;

        let unmerged = self.get_working_tree_conflicts()?;
        if !unmerged.is_empty() {
            return Err(DeliveryError::ConflictResolutionFailed(format!(
                "Still have {} unmerged files",
                unmerged.len()
            )));
        }

        self.git(&["add", "-A"])?;
        self.git(&["commit", "-m", commit_message])?;

        let sha = self.git(&["rev-parse", "HEAD"])?;
        info!("Completed merge commit: {}", sha);

        Ok(sha)
    }

    /// Abort an in-progress merge
    pub fn abort_merge(&self) -> DeliveryResult<()> {
        self.git(&["merge", "--abort"])?;
        info!("Aborted merge");
        Ok(())
    }

    /// List remote branches (strips `origin/` prefix, excludes HEAD)
    pub fn list_remote_branches(&self) -> DeliveryResult<Vec<String>> {
        let output = self.git(&["branch", "-r"])?;
        let branches: Vec<String> = output
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.contains("HEAD"))
            .filter_map(|line| line.strip_prefix("origin/"))
            .map(|s| s.to_string())
            .collect();
        Ok(branches)
    }

    /// Make a URL to view a branch on GitHub
    fn make_branch_url(&self, remote_url: &str, branch: &str) -> Option<String> {
        use crate::core::forge::parse_github_remote_url;

        let repo = parse_github_remote_url(remote_url)?;
        Some(format!("https://github.com/{}/tree/{}", repo, branch))
    }
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
