//! Git operations for hirsel
//!
//! Provides git repository management, worktree operations, branch management,
//! and merge/diff utilities. Uses git2 crate for native git operations.

use git2::{
    BranchType, Error as Git2Error, FetchOptions, Oid, RemoteCallbacks, Repository, Signature,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
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

/// Check whether a specific branch exists on a remote repository.
pub fn remote_branch_exists(url: &str, branch: &str) -> Result<bool> {
    let ref_name = format!("refs/heads/{}", branch);
    let output = Command::new("git")
        .args(["ls-remote", "--exit-code", "--heads", url, &ref_name])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env(
            "GIT_SSH_COMMAND",
            "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new",
        )
        .output()
        .map_err(|e| GitError::Other(format!("Failed to run git ls-remote: {}", e)))?;

    if output.status.success() {
        return Ok(true);
    }

    match output.status.code() {
        Some(2) => Ok(false),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(GitError::Other(format!("git ls-remote failed: {}", stderr)))
        }
    }
}

/// Best-effort detection of the remote's default branch via HEAD symref.
pub fn remote_default_branch(url: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["ls-remote", "--symref", url, "HEAD"])
        .env("GIT_TERMINAL_PROMPT", "0")
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
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("ref: refs/heads/") {
            if let Some(branch) = rest.strip_suffix("\tHEAD") {
                return Ok(Some(branch.to_string()));
            }
        }
    }

    Ok(None)
}

/// Check whether a remote branch contains a specific file at the repository root.
pub fn remote_branch_has_file(url: &str, branch: &str, path: &str) -> Result<bool> {
    let temp =
        TempDir::new().map_err(|e| GitError::Other(format!("Failed to create temp dir: {}", e)))?;
    let auth_url = get_authenticated_url(url);

    run_git_command(
        temp.path(),
        &[
            "clone",
            "--depth",
            "1",
            "--filter=blob:none",
            "--branch",
            branch,
            "--single-branch",
            &auth_url,
            ".",
        ],
    )?;

    Ok(temp.path().join(path).is_file())
}

/// Check whether a remote branch contains a root `flake.nix`.
pub fn remote_branch_has_flake(url: &str, branch: &str) -> Result<bool> {
    remote_branch_has_file(url, branch, "flake.nix")
}

fn run_git_command(current_dir: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(current_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env(
            "GIT_SSH_COMMAND",
            "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new",
        )
        .output()
        .map_err(|e| GitError::Other(format!("Failed to run git {:?}: {}", args, e)))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "unknown git error".to_string()
    };
    Err(GitError::Other(format!(
        "git {:?} failed: {}",
        args, detail
    )))
}

/// Create a missing remote branch, either from an existing visible branch or by
/// initializing an empty remote repository with an empty initial commit.
pub fn create_remote_branch(url: &str, branch: &str, from_branch: Option<&str>) -> Result<()> {
    if remote_branch_exists(url, branch)? {
        return Ok(());
    }

    let auth_url = get_authenticated_url(url);

    if let Some(base_branch) = from_branch {
        let temp = TempDir::new()
            .map_err(|e| GitError::Other(format!("Failed to create temp dir: {}", e)))?;
        run_git_command(
            temp.path(),
            &[
                "clone",
                "--branch",
                base_branch,
                "--single-branch",
                &auth_url,
                ".",
            ],
        )?;
        run_git_command(temp.path(), &["checkout", "-b", branch])?;
        run_git_command(temp.path(), &["push", "-u", "origin", branch])?;
        return Ok(());
    }

    let temp =
        TempDir::new().map_err(|e| GitError::Other(format!("Failed to create temp dir: {}", e)))?;
    run_git_command(temp.path(), &["init", "--initial-branch", branch])?;
    run_git_command(temp.path(), &["config", "user.name", "Hirsel"])?;
    run_git_command(
        temp.path(),
        &["config", "user.email", "hirsel@localhost.localdomain"],
    )?;
    run_git_command(
        temp.path(),
        &[
            "commit",
            "--allow-empty",
            "-m",
            "Initialize repository for Hirsel",
        ],
    )?;
    run_git_command(temp.path(), &["remote", "add", "origin", &auth_url])?;
    run_git_command(temp.path(), &["push", "-u", "origin", branch])?;
    Ok(())
}

/// Get the authenticated URL for a git remote
///
/// If a GitHub token is configured in environment, embeds it in HTTPS URLs.
/// SSH URLs are returned unchanged.
fn get_authenticated_url(url: &str) -> String {
    // Only modify HTTPS GitHub URLs
    if !url.starts_with("https://github.com/") {
        return url.to_string();
    }

    let token = std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok()
        .filter(|value| !value.trim().is_empty());

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

fn build_git_remote_callbacks() -> RemoteCallbacks<'static> {
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|url, username_from_url, allowed_types| {
        tracing::debug!(
            "Git credentials requested for URL: {:?}, username: {:?}, allowed_types: {:?}",
            url,
            username_from_url,
            allowed_types
        );

        if allowed_types.contains(git2::CredentialType::USERNAME) {
            if let Some(username) = username_from_url {
                tracing::debug!("Trying username credential '{}'", username);
                match git2::Cred::username(username) {
                    Ok(cred) => return Ok(cred),
                    Err(e) => tracing::debug!("Username credential failed: {}", e),
                }
            }
        }

        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            if let Some(username) = username_from_url {
                tracing::debug!("Trying SSH agent for user '{}'", username);
                match git2::Cred::ssh_key_from_agent(username) {
                    Ok(cred) => return Ok(cred),
                    Err(e) => tracing::debug!("SSH agent failed: {}", e),
                }
            }
        }

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
            }
        }

        if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            tracing::debug!("Trying git credential helper");
            let config = git2::Config::open_default()?;
            match git2::Cred::credential_helper(&config, url, username_from_url) {
                Ok(cred) => return Ok(cred),
                Err(e) => tracing::debug!("Credential helper failed: {}", e),
            }
        }

        tracing::warn!("No git credentials available for clone/fetch");
        Err(git2::Error::from_str(
            "no credentials available - check SSH keys, git credential helper, or GitHub token",
        ))
    });
    callbacks
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
        return Err(GitError::Other(format!(
            "Clone target already exists: {}",
            target_dir.display()
        )));
    }

    fs::create_dir_all(target_dir)?;

    info!("Cloning {} to {:?}", url, target_dir);

    // Use authenticated URL if token is available
    let auth_url = get_authenticated_url(url);
    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(build_git_remote_callbacks());

    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_options);
    if let Some(branch_name) = branch {
        info!("Selecting branch during clone: {}", branch_name);
        builder.branch(branch_name);
    }
    let repo = builder.clone(&auth_url, target_dir)?;

    // Ensure we have a working directory
    let work_dir = repo
        .workdir()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| GitError::Other("Cloned repository has no working directory".to_string()))?;

    info!("Successfully cloned to {:?}", work_dir);
    Ok(work_dir)
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

/// Create the main workspace directory with the central checkout.
///
/// Copies the project to `workspaces/<workspace_name>/work/central/` with full git history,
/// creates the `central` branch, and allows thread checkouts to fetch from it.
pub fn create_workspace(
    workspace_name: &str,
    project_path: &Path,
    workspaces_dir: &Path,
) -> Result<PathBuf> {
    let workspace_dir = workspaces_dir.join(workspace_name);
    let central_dir = workspace_dir.join("work").join("central");

    if central_dir.exists() {
        info!("Workspace already exists: {:?}", central_dir);
        return Ok(central_dir);
    }

    fs::create_dir_all(&workspace_dir)?;

    // Copy entire project including .git
    copy_dir_recursive(project_path, &central_dir)?;
    info!("Copied project to: {:?}", central_dir);

    let repo = Repository::open(&central_dir)?;

    // Commit any uncommitted changes first
    if is_dirty(&repo)? {
        add_all(&repo)?;
        commit(&repo, "hirsel: snapshot uncommitted changes")?;
    }

    // Force create "central" branch from current HEAD (whatever branch we're on)
    // This ensures we capture the current state regardless of source branch name
    let head_commit = repo.head()?.peel_to_commit()?;

    // Get current branch name before we modify anything
    let current_branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()));

    // Delete existing central branch if present (it might have different content)
    if let Ok(mut branch) = repo.find_branch("central", BranchType::Local) {
        // Can't delete current branch, so only delete if we're not on it
        if current_branch.as_deref() != Some("central") {
            let _ = branch.delete();
        }
    }

    // Create central branch from HEAD (skip if we're already on central)
    if current_branch.as_deref() != Some("central") {
        repo.branch("central", &head_commit, false)?;
    }

    // Checkout central branch
    let obj = repo.revparse_single("central")?;
    repo.checkout_tree(&obj, None)?;
    repo.set_head("refs/heads/central")?;

    // Delete all other branches except central
    let branches: Vec<String> = repo
        .branches(Some(BranchType::Local))?
        .filter_map(|b| b.ok())
        .filter_map(|(b, _)| b.name().ok().flatten().map(|s| s.to_string()))
        .filter(|n| n != "central")
        .collect();

    for branch_name in branches {
        if let Ok(mut branch) = repo.find_branch(&branch_name, BranchType::Local) {
            let _ = branch.delete();
        }
    }

    // Allow thread checkouts to update from this repo if they use it as a local remote.
    let mut config = repo.config()?;
    config.set_str("receive.denyCurrentBranch", "updateInstead")?;

    info!("Created workspace on 'central' branch: {:?}", central_dir);
    Ok(central_dir)
}

/// Create a thread checkout that tracks the central checkout.
///
/// Copies the full central checkout snapshot to
/// `workspaces/<workspace_name>/work/<checkout_name>/`, preserves any dirty or
/// untracked working tree files, and switches the copy onto a dedicated local
/// thread branch while wiring `origin` back to the central checkout.
pub fn create_thread_checkout(
    workspace_name: &str,
    project_path: &Path,
    checkout_name: &str,
    central_dir: Option<&Path>,
    workspaces_dir: &Path,
) -> Result<PathBuf> {
    let workspace_dir = workspaces_dir.join(workspace_name);
    let central = central_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| workspace_dir.join("work").join("central"));
    let checkout_dir = workspace_dir.join("work").join(checkout_name);

    if checkout_dir.exists() {
        info!("Thread checkout already exists: {:?}", checkout_dir);
        return Ok(checkout_dir);
    }

    if !central.join(".git").is_dir() {
        return Err(GitError::Other(format!(
            "Main workspace not initialized: {:?}",
            central
        )));
    }

    // Copy the full central snapshot, including .git and any untracked files.
    copy_dir_recursive(project_path, &checkout_dir)?;
    info!(
        "Copied central snapshot to thread checkout: {:?}",
        checkout_dir
    );

    let repo = Repository::open(&checkout_dir)?;
    let head_commit = repo.head()?.peel_to_commit()?;
    repo.branch(checkout_name, &head_commit, true)?;
    repo.set_head(&format!("refs/heads/{checkout_name}"))?;

    let central_url = central.canonicalize()?.display().to_string();
    if repo.find_remote("origin").is_ok() {
        repo.remote_set_url("origin", &central_url)?;
    } else {
        repo.remote("origin", &central_url)?;
    }
    if let Ok(remotes) = repo.remotes() {
        for remote_name in remotes.iter().flatten().filter(|name| *name != "origin") {
            let _ = repo.remote_delete(remote_name);
        }
    }

    let mut remote = repo.find_remote("origin")?;
    remote.fetch(&["central"], None, None)?;

    if repo.find_reference("refs/remotes/origin/central").is_ok() {
        let mut branch = repo.find_branch(checkout_name, BranchType::Local)?;
        branch.set_upstream(Some("origin/central"))?;
    }

    info!(
        "Created thread checkout snapshot: {:?} (branch {} tracking {:?})",
        checkout_dir, checkout_name, central
    );
    Ok(checkout_dir)
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
    fn create_thread_checkout_preserves_untracked_central_files() {
        let (source_dir, _repo) = create_test_repo();
        let workspaces = TempDir::new().unwrap();
        let central_dir =
            create_workspace("project-1", source_dir.path(), workspaces.path()).unwrap();

        std::fs::write(central_dir.join("flake.nix"), "{ }").unwrap();

        let checkout_dir = create_thread_checkout(
            "project-1",
            &central_dir,
            "thread-smoke",
            Some(&central_dir),
            workspaces.path(),
        )
        .unwrap();

        assert!(
            checkout_dir.join("flake.nix").is_file(),
            "thread checkout lost the untracked central flake snapshot"
        );
        assert_eq!(
            get_current_branch(&checkout_dir).unwrap(),
            "thread-smoke".to_string()
        );

        let repo = Repository::open(&checkout_dir).unwrap();
        let origin = repo.find_remote("origin").unwrap();
        assert_eq!(
            origin.url().unwrap(),
            central_dir.canonicalize().unwrap().display().to_string()
        );
    }
}
