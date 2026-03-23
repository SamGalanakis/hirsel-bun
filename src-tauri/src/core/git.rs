//! Git operations for hirsel
//!
//! Provides git repository management, worktree operations, branch management,
//! and merge/diff utilities. Uses git2 crate for native git operations.

use git2::{BranchType, Error as Git2Error, Oid, Repository, Signature};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use tracing::info;

/// Git operation errors
#[derive(Error, Debug)]
pub enum GitError {
    #[error("Not a git repository: {0}")]
    NotARepository(PathBuf),

    #[error("Git operation failed: {0}")]
    Git2(#[from] Git2Error),

    #[error("Branch not found: {0}")]
    BranchNotFound(String),

    #[error("Merge conflict in files: {0:?}")]
    MergeConflict(Vec<String>),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Push failed: {0}")]
    PushFailed(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, GitError>;

// =============================================================================
// Remote URL Detection and Cloning
// =============================================================================

/// Check if a string is a remote git URL
///
/// Supports:
/// - HTTPS: https://github.com/user/repo.git
/// - SSH: git@github.com:user/repo.git
/// - Git protocol: git://github.com/user/repo.git
pub fn is_remote_url(path: &str) -> bool {
    let trimmed = path.trim();
    trimmed.starts_with("https://")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("git://")
        || trimmed.starts_with("git@")
        || trimmed.starts_with("ssh://")
}

/// Result of parsing a GitHub URL
#[derive(Debug, Clone)]
pub struct ParsedRepoUrl {
    /// The base repository URL (without branch path)
    pub repo_url: String,
    /// The branch name if specified in the URL (e.g., from /tree/branch-name)
    pub branch: Option<String>,
}

/// Parse a GitHub URL and extract the base repo URL and optional branch
///
/// Handles URLs like:
/// - https://github.com/user/repo
/// - https://github.com/user/repo/tree/branch-name
/// - https://github.com/user/repo/tree/feature/nested-branch
/// - git@github.com:user/repo.git
pub fn parse_github_url(url: &str) -> ParsedRepoUrl {
    let trimmed = url.trim();

    // Handle SSH format - no branch extraction possible
    if trimmed.starts_with("git@") || trimmed.starts_with("ssh://") {
        return ParsedRepoUrl {
            repo_url: trimmed.to_string(),
            branch: None,
        };
    }

    // Handle HTTPS GitHub URLs with /tree/branch pattern
    if let Some(tree_idx) = trimmed.find("/tree/") {
        let repo_url = trimmed[..tree_idx].to_string();
        let branch = trimmed[tree_idx + 6..].to_string(); // Skip "/tree/"

        // Remove trailing slashes from branch
        let branch = branch.trim_end_matches('/').to_string();

        return ParsedRepoUrl {
            repo_url,
            branch: if branch.is_empty() {
                None
            } else {
                Some(branch)
            },
        };
    }

    // Handle /blob/branch pattern (less common but valid)
    if let Some(blob_idx) = trimmed.find("/blob/") {
        let repo_url = trimmed[..blob_idx].to_string();
        // Extract just the branch part (before any file path)
        let rest = &trimmed[blob_idx + 6..];
        let branch = rest.split('/').next().unwrap_or("").to_string();

        return ParsedRepoUrl {
            repo_url,
            branch: if branch.is_empty() {
                None
            } else {
                Some(branch)
            },
        };
    }

    // No branch in URL
    ParsedRepoUrl {
        repo_url: trimmed.trim_end_matches('/').to_string(),
        branch: None,
    }
}

/// List branches from a remote repository
///
/// Uses git ls-remote to fetch branch names without cloning.
/// Sets environment variables to prevent hanging on credential prompts.
pub fn list_remote_branches(url: &str) -> Result<Vec<String>> {
    use std::process::Command;

    let output = Command::new("git")
        .args(["ls-remote", "--heads", url])
        // Prevent git from prompting for credentials (would hang)
        .env("GIT_TERMINAL_PROMPT", "0")
        // Prevent SSH from prompting for passwords (would hang)
        .env(
            "GIT_SSH_COMMAND",
            "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new",
        )
        .output()
        .map_err(|e| GitError::Other(format!("Failed to run git ls-remote: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GitError::Other(format!("git ls-remote failed: {}", stderr)));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut branches: Vec<String> = stdout
        .lines()
        .filter_map(|line| {
            // Format: <sha>\trefs/heads/<branch-name>
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                parts[1].strip_prefix("refs/heads/").map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect();

    branches.sort();
    Ok(branches)
}

/// Get the authenticated URL for a git remote
///
/// If a GitHub token is configured in CredentialStore, embeds it in HTTPS URLs.
/// SSH URLs are returned unchanged.
fn get_authenticated_url(url: &str) -> String {
    use crate::core::credentials::CredentialStore;

    // Only modify HTTPS GitHub URLs
    if !url.starts_with("https://github.com/") {
        return url.to_string();
    }

    // Try to load GitHub token from credential store
    // Use a runtime since this function is sync but CredentialStore is now async
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return url.to_string(),
    };

    let token = rt.block_on(async {
        let store = CredentialStore::open().await.ok()?;
        store.load("git_github_token").await.ok()
    });

    if let Some(token) = token {
        // Embed token in URL: https://TOKEN@github.com/user/repo.git
        return url.replacen(
            "https://github.com/",
            &format!("https://{}@github.com/", token),
            1,
        );
    }

    url.to_string()
}

/// Clone a remote repository to a local directory with optional branch checkout
///
/// If branch is specified, checks out that branch after cloning.
/// Returns the path to the cloned repository.
/// Uses GitHub token from CredentialStore for authentication if available.
pub fn clone_remote_with_branch(
    url: &str,
    target_dir: &Path,
    branch: Option<&str>,
) -> Result<PathBuf> {
    if target_dir.exists() {
        info!("Clone target already exists: {:?}", target_dir);
        return Ok(target_dir.to_path_buf());
    }

    fs::create_dir_all(target_dir)?;

    info!("Cloning {} to {:?}", url, target_dir);

    // Use authenticated URL if token is available
    let auth_url = get_authenticated_url(url);
    let repo = Repository::clone(&auth_url, target_dir)?;

    // Checkout specific branch if requested
    if let Some(branch_name) = branch {
        info!("Checking out branch: {}", branch_name);
        checkout_branch(&repo, branch_name)?;
    }

    // Ensure we have a working directory
    let work_dir = repo
        .workdir()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| GitError::Other("Cloned repository has no working directory".to_string()))?;

    info!("Successfully cloned to {:?}", work_dir);
    Ok(work_dir)
}

/// Push staging branch to a remote repository
///
/// Creates or updates a branch on the remote.
/// Uses GitHub token from CredentialStore for authentication if available.
pub fn push_to_remote(
    work_dir: &Path,
    remote_url: &str,
    branch_name: &str,
) -> Result<(bool, String)> {
    let repo = get_repo(Some(work_dir))?;

    // Make sure we're on staging
    checkout_branch(&repo, "staging")?;

    // Add or update the remote with authenticated URL
    let remote_name = "hirsel_delivery";
    let auth_url = get_authenticated_url(remote_url);

    // Remove existing remote if present
    let _ = repo.remote_delete(remote_name);

    repo.remote(remote_name, &auth_url)?;

    // Push staging as the target branch
    let mut remote = repo.find_remote(remote_name)?;

    let refspec = format!("refs/heads/staging:refs/heads/{}", branch_name);

    // Use default push options
    let mut push_opts = git2::PushOptions::new();

    // Set up credentials callback for SSH/HTTPS auth
    let mut callbacks = git2::RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, allowed_types| {
        tracing::debug!(
            "Git credentials requested for URL: {:?}, username: {:?}, allowed_types: {:?}",
            _url,
            username_from_url,
            allowed_types
        );

        // Try SSH agent first
        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            if let Some(username) = username_from_url {
                tracing::debug!("Trying SSH agent for user '{}'", username);
                match git2::Cred::ssh_key_from_agent(username) {
                    Ok(cred) => return Ok(cred),
                    Err(e) => tracing::debug!("SSH agent failed: {}", e),
                }
            }
        }

        // Try default SSH key
        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            let ssh_key = std::path::PathBuf::from(&home).join(".ssh/id_rsa");
            let ssh_key_ed = std::path::PathBuf::from(&home).join(".ssh/id_ed25519");

            let key_path = if ssh_key_ed.exists() {
                ssh_key_ed
            } else {
                ssh_key
            };

            if key_path.exists() {
                let username = username_from_url.unwrap_or("git");
                tracing::debug!("Trying SSH key at {:?} for user '{}'", key_path, username);
                match git2::Cred::ssh_key(username, None, &key_path, None) {
                    Ok(cred) => return Ok(cred),
                    Err(e) => tracing::debug!("SSH key failed: {}", e),
                }
            } else {
                tracing::debug!("No SSH key found at {:?}", key_path);
            }
        }

        // Try git credential helper for HTTPS
        if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            tracing::debug!("Trying git credential helper");
            match git2::Cred::credential_helper(&repo.config()?, _url, username_from_url) {
                Ok(cred) => return Ok(cred),
                Err(e) => tracing::debug!("Credential helper failed: {}", e),
            }
        }

        tracing::warn!("No git credentials available - authentication will fail");
        Err(git2::Error::from_str(
            "no credentials available - check SSH keys or git credential helper",
        ))
    });

    push_opts.remote_callbacks(callbacks);

    match remote.push(&[&refspec], Some(&mut push_opts)) {
        Ok(()) => {
            // Clean up remote
            let _ = repo.remote_delete(remote_name);
            info!("Pushed to remote {} as branch {}", remote_url, branch_name);
            Ok((
                true,
                format!("Pushed to branch '{}' on remote", branch_name),
            ))
        }
        Err(e) => {
            let _ = repo.remote_delete(remote_name);
            // Parse error to provide better user feedback
            let error_str = e.to_string();
            let user_message = if error_str.contains("authentication")
                || error_str.contains("credential")
                || error_str.contains("permission denied")
                || error_str.contains("publickey")
            {
                format!(
                    "Git authentication failed: {}. Check your SSH keys or git credentials.",
                    error_str
                )
            } else if error_str.contains("could not read")
                || error_str.contains("network")
                || error_str.contains("connection")
            {
                format!(
                    "Git network error: {}. Check your internet connection.",
                    error_str
                )
            } else {
                format!("Git push failed: {}", error_str)
            };
            Err(GitError::PushFailed(user_message))
        }
    }
}

// =============================================================================
// Repository Operations
// =============================================================================

/// Open a git repository, searching parent directories if needed
pub fn get_repo(cwd: Option<&Path>) -> Result<Repository> {
    let path = cwd
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    Repository::discover(&path).map_err(|_| GitError::NotARepository(path))
}

/// Check if a branch exists in the repository
pub fn branch_exists(branch: &str, cwd: Option<&Path>) -> Result<bool> {
    let repo = get_repo(cwd)?;
    let exists = repo.find_branch(branch, BranchType::Local).is_ok();
    Ok(exists)
}

/// Get the current branch name
pub fn get_current_branch(work_dir: &Path) -> Result<String> {
    let repo = get_repo(Some(work_dir))?;
    let head = repo.head()?;

    if head.is_branch() {
        head.shorthand()
            .map(|s| s.to_string())
            .ok_or_else(|| GitError::Other("Could not get branch name".to_string()))
    } else {
        Err(GitError::Other("HEAD is not on a branch".to_string()))
    }
}

/// List all local branches
pub fn list_branches(work_dir: &Path) -> Result<Vec<String>> {
    let repo = get_repo(Some(work_dir))?;
    let branches = repo.branches(Some(BranchType::Local))?;

    let mut names = Vec::new();
    for branch_result in branches {
        let (branch, _) = branch_result?;
        if let Some(name) = branch.name()? {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

/// List branches that are not yet merged into staging
pub fn list_unmerged_branches(work_dir: &Path) -> Result<Vec<String>> {
    let repo = get_repo(Some(work_dir))?;
    let branches = repo.branches(Some(BranchType::Local))?;

    // Get staging commit
    let staging_branch = repo
        .find_branch("staging", BranchType::Local)
        .map_err(|_| GitError::BranchNotFound("staging".to_string()))?;
    let staging_commit = staging_branch.get().peel_to_commit()?;

    let mut unmerged = Vec::new();
    for branch_result in branches {
        let (branch, _) = branch_result?;
        if let Some(name) = branch.name()? {
            // Only check task/ branches
            if !name.starts_with("task/") {
                continue;
            }

            let branch_commit = branch.get().peel_to_commit()?;

            // Check if branch is merged: merge-base equals branch head means merged
            let merge_base = repo.merge_base(staging_commit.id(), branch_commit.id())?;

            if merge_base != branch_commit.id() {
                unmerged.push(name.to_string());
            }
        }
    }

    Ok(unmerged)
}

/// Create the main workspace directory with staging branch
///
/// Copies the project to `runtimes/<runtime_name>/work/staging/` with full git history,
/// creates "staging" branch, and sets up receive.denyCurrentBranch for worker pushes.
pub fn create_workspace(
    runtime_name: &str,
    project_path: &Path,
    runtimes_dir: &Path,
) -> Result<PathBuf> {
    let runtime_dir = runtimes_dir.join(runtime_name);
    let staging_dir = runtime_dir.join("work").join("staging");

    if staging_dir.exists() {
        info!("Workspace already exists: {:?}", staging_dir);
        return Ok(staging_dir);
    }

    fs::create_dir_all(&runtime_dir)?;

    // Copy entire project including .git
    copy_dir_recursive(project_path, &staging_dir)?;
    info!("Copied project to: {:?}", staging_dir);

    let repo = Repository::open(&staging_dir)?;

    // Commit any uncommitted changes first
    if is_dirty(&repo)? {
        add_all(&repo)?;
        commit(&repo, "hirsel: snapshot uncommitted changes")?;
    }

    // Force create "staging" branch from current HEAD (whatever branch we're on)
    // This ensures we capture the current state regardless of source branch name
    let head_commit = repo.head()?.peel_to_commit()?;

    // Get current branch name before we modify anything
    let current_branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()));

    // Delete existing staging branch if present (it might have different content)
    if let Ok(mut branch) = repo.find_branch("staging", BranchType::Local) {
        // Can't delete current branch, so only delete if we're not on it
        if current_branch.as_deref() != Some("staging") {
            let _ = branch.delete();
        }
    }

    // Create staging branch from HEAD (skip if we're already on staging)
    if current_branch.as_deref() != Some("staging") {
        repo.branch("staging", &head_commit, false)?;
    }

    // Checkout staging branch
    let obj = repo.revparse_single("staging")?;
    repo.checkout_tree(&obj, None)?;
    repo.set_head("refs/heads/staging")?;

    // Delete all other branches except staging
    let branches: Vec<String> = repo
        .branches(Some(BranchType::Local))?
        .filter_map(|b| b.ok())
        .filter_map(|(b, _)| b.name().ok().flatten().map(|s| s.to_string()))
        .filter(|n| n != "staging")
        .collect();

    for branch_name in branches {
        if let Ok(mut branch) = repo.find_branch(&branch_name, BranchType::Local) {
            let _ = branch.delete();
        }
    }

    // Allow workers to push to this repo
    let mut config = repo.config()?;
    config.set_str("receive.denyCurrentBranch", "updateInstead")?;

    info!("Created workspace on 'staging' branch: {:?}", staging_dir);
    Ok(staging_dir)
}

/// Create a worker clone that tracks staging
///
/// Copies project files to `runtimes/<runtime_name>/work/<worker_name>/`, initializes
/// a fresh git repo, and sets up origin pointing to staging_dir.
pub fn create_worker_clone(
    runtime_name: &str,
    project_path: &Path,
    worker_name: &str,
    staging_dir: Option<&Path>,
    runtimes_dir: &Path,
) -> Result<PathBuf> {
    let runtime_dir = runtimes_dir.join(runtime_name);
    let staging = staging_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| runtime_dir.join("work").join("staging"));
    let worker_dir = runtime_dir.join("work").join(worker_name);

    if worker_dir.exists() {
        info!("Worker clone already exists: {:?}", worker_dir);
        return Ok(worker_dir);
    }

    if !staging.join(".git").is_dir() {
        return Err(GitError::Other(format!(
            "Main workspace not initialized: {:?}",
            staging
        )));
    }

    // Copy ALL project files (including untracked)
    copy_dir_recursive(project_path, &worker_dir)?;
    info!("Copied project files to worker dir: {:?}", worker_dir);

    // Remove .git from copied directory
    let copied_git = worker_dir.join(".git");
    if copied_git.is_dir() {
        fs::remove_dir_all(&copied_git)?;
    } else if copied_git.exists() {
        fs::remove_file(&copied_git)?;
    }

    // Initialize fresh git repo
    let repo = Repository::init(&worker_dir)?;

    // Create initial commit on a temporary branch
    {
        let sig = get_signature(&repo)?;
        let tree_id = {
            let mut index = repo.index()?;
            index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
            index.write()?;
            index.write_tree()?
        };
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;
    }

    // Rename the default branch to 'staging'
    // git2 doesn't have a rename API, so we create staging and delete the old branch
    let head_commit = repo.head()?.peel_to_commit()?;
    repo.branch("staging", &head_commit, false)?;
    repo.set_head("refs/heads/staging")?;

    // Delete the old default branch (master/main)
    if let Ok(mut head_ref) = repo.find_reference("refs/heads/master") {
        let _ = head_ref.delete();
    }
    if let Ok(mut head_ref) = repo.find_reference("refs/heads/main") {
        let _ = head_ref.delete();
    }

    // Add staging as origin remote
    let staging_url = staging.canonicalize()?.display().to_string();
    repo.remote("origin", &staging_url)?;

    // Fetch from origin
    let mut remote = repo.find_remote("origin")?;
    remote.fetch(&["staging"], None, None)?;

    // Reset to origin/staging
    let origin_staging = repo.find_reference("refs/remotes/origin/staging")?;
    let commit = origin_staging.peel_to_commit()?;
    repo.reset(commit.as_object(), git2::ResetType::Hard, None)?;

    // Set upstream tracking
    let mut branch = repo.find_branch("staging", BranchType::Local)?;
    branch.set_upstream(Some("origin/staging"))?;

    // Copy git config from staging (user.name, user.email)
    if let Ok(staging_repo) = Repository::open(&staging) {
        if let Ok(staging_config) = staging_repo.config() {
            let mut worker_config = repo.config()?;
            for key in ["user.name", "user.email"] {
                if let Ok(value) = staging_config.get_string(key) {
                    let _ = worker_config.set_str(key, &value);
                }
            }
        }
    }

    info!(
        "Created worker clone: {:?} (origin → {:?})",
        worker_dir, staging
    );
    Ok(worker_dir)
}

/// Get diff stat between project and work directory
pub fn get_diff_stat(project_path: &Path, work_dir: &Path) -> Result<Option<String>> {
    diff_between_repos(project_path, work_dir, true)
}

/// Get full diff between project and work directory
pub fn get_diff(project_path: &Path, work_dir: &Path) -> Result<Option<String>> {
    diff_between_repos(project_path, work_dir, false)
}

/// Push staging branch as a new branch to project repo
pub fn push_staging_as_branch(
    work_dir: &Path,
    project_path: &Path,
    branch_name: &str,
) -> Result<(bool, String)> {
    let work_repo = get_repo(Some(work_dir))?;
    let project_repo = get_repo(Some(project_path))?;

    // Make sure we're on staging
    checkout_branch(&work_repo, "staging")?;

    // Add work_dir as temporary remote
    let remote_name = "hirsel_work";
    let work_url = work_dir.canonicalize()?.display().to_string();

    // Remove existing remote if present
    let _ = project_repo.remote_delete(remote_name);

    project_repo.remote(remote_name, &work_url)?;

    // Fetch from work dir
    let mut remote = project_repo.find_remote(remote_name)?;
    remote.fetch(&["staging"], None, None)?;

    // Delete branch if it exists
    if let Ok(mut branch) = project_repo.find_branch(branch_name, BranchType::Local) {
        branch.delete()?;
    }

    // Create the new branch from fetched staging
    let remote_ref =
        project_repo.find_reference(&format!("refs/remotes/{}/staging", remote_name))?;
    let commit = remote_ref.peel_to_commit()?;
    project_repo.branch(branch_name, &commit, false)?;

    // Clean up remote
    project_repo.remote_delete(remote_name)?;

    info!("Created branch '{}' in project repo", branch_name);
    Ok((
        true,
        format!(
            "Created branch '{}'. Review and merge when ready.",
            branch_name
        ),
    ))
}

// =============================================================================
// Helper functions
// =============================================================================

/// Copy directory recursively
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if copy_dir_reflink(src, dst).is_ok() {
        return Ok(());
    }

    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

fn copy_dir_reflink(src: &Path, dst: &Path) -> std::io::Result<()> {
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

/// Check if repository has uncommitted changes
fn is_dirty(repo: &Repository) -> Result<bool> {
    let statuses = repo.statuses(None)?;
    Ok(!statuses.is_empty())
}

/// Add all files to index
fn add_all(repo: &Repository) -> Result<()> {
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;
    Ok(())
}

/// Create a commit
fn commit(repo: &Repository, message: &str) -> Result<Oid> {
    let sig = get_signature(repo)?;
    let tree_id = repo.index()?.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let parent = repo.head()?.peel_to_commit()?;
    let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])?;

    Ok(oid)
}

/// Get signature for commits
fn get_signature(repo: &Repository) -> Result<Signature<'static>> {
    // Try to get from repo config first
    if let Ok(config) = repo.config() {
        let name = config
            .get_string("user.name")
            .unwrap_or_else(|_| "hirsel".to_string());
        let email = config
            .get_string("user.email")
            .unwrap_or_else(|_| "hirsel@localhost".to_string());
        return Ok(Signature::now(&name, &email)?);
    }

    // Fallback
    Ok(Signature::now("hirsel", "hirsel@localhost")?)
}

/// Checkout a branch
fn checkout_branch(repo: &Repository, branch_name: &str) -> Result<()> {
    let obj = repo.revparse_single(&format!("refs/heads/{}", branch_name))?;
    repo.checkout_tree(&obj, None)?;
    repo.set_head(&format!("refs/heads/{}", branch_name))?;
    Ok(())
}

/// Generate diff between two repositories
fn diff_between_repos(
    project_path: &Path,
    work_dir: &Path,
    stat_only: bool,
) -> Result<Option<String>> {
    let project_repo = get_repo(Some(project_path))?;
    let work_branch = get_current_branch(work_dir)?;

    // Add work_dir as temporary remote
    let remote_name = "hirsel_work";
    let work_url = work_dir.canonicalize()?.display().to_string();

    // Remove existing remote if present
    let _ = project_repo.remote_delete(remote_name);

    project_repo.remote(remote_name, &work_url)?;

    // Fetch from work dir
    let mut remote = project_repo.find_remote(remote_name)?;
    remote.fetch(&[&work_branch], None, None)?;

    // Get trees for diff
    let head_tree = project_repo.head()?.peel_to_tree()?;
    let remote_ref =
        project_repo.find_reference(&format!("refs/remotes/{}/{}", remote_name, work_branch))?;
    let remote_tree = remote_ref.peel_to_tree()?;

    // Generate diff
    let diff = project_repo.diff_tree_to_tree(Some(&head_tree), Some(&remote_tree), None)?;

    let result = if stat_only {
        let stats = diff.stats()?;
        Some(format!(
            "{} files changed, {} insertions(+), {} deletions(-)",
            stats.files_changed(),
            stats.insertions(),
            stats.deletions()
        ))
    } else {
        let mut diff_str = String::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            let prefix = match line.origin() {
                '+' => "+",
                '-' => "-",
                ' ' => " ",
                _ => "",
            };
            if let Ok(content) = std::str::from_utf8(line.content()) {
                diff_str.push_str(prefix);
                diff_str.push_str(content);
            }
            true
        })?;

        if diff_str.is_empty() {
            None
        } else {
            Some(diff_str)
        }
    };

    // Clean up remote
    project_repo.remote_delete(remote_name)?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_repo() -> (TempDir, Repository) {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        // Create initial commit
        let sig = Signature::now("test", "test@test.com").unwrap();
        {
            let mut index = repo.index().unwrap();

            // Create a test file
            let file_path = dir.path().join("test.txt");
            let mut file = File::create(&file_path).unwrap();
            writeln!(file, "test content").unwrap();

            index.add_path(Path::new("test.txt")).unwrap();
            index.write().unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        (dir, repo)
    }

    #[test]
    fn test_get_repo() {
        let (dir, _repo) = create_test_repo();
        let result = get_repo(Some(dir.path()));
        assert!(result.is_ok());
    }

    #[test]
    fn test_branch_exists() {
        let (dir, repo) = create_test_repo();

        // Create a test branch
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("test-branch", &head, false).unwrap();

        assert!(branch_exists("test-branch", Some(dir.path())).unwrap());
        assert!(!branch_exists("nonexistent", Some(dir.path())).unwrap());
    }

    #[test]
    fn test_list_branches() {
        let (dir, repo) = create_test_repo();

        // Create some branches
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature-1", &head, false).unwrap();
        repo.branch("feature-2", &head, false).unwrap();

        let branches = list_branches(dir.path()).unwrap();
        assert!(branches.contains(&"master".to_string()) || branches.contains(&"main".to_string()));
        assert!(branches.contains(&"feature-1".to_string()));
        assert!(branches.contains(&"feature-2".to_string()));
    }
}
