//! Per-thread workspace worktrees.
//!
//! Each thread with `workspace_write` in its caps gets its own
//! **git worktree** on a thread-scoped branch (`hirsel-thread/{id}`).
//! The worktree is a real, checked-out copy of the canonical workspace
//! — tools, shells, editors, and reviewers can all operate inside it
//! without touching the canonical state.
//!
//! Merging is `git merge hirsel-thread/{id}` in the canonical workspace.
//! Conflicts leave canonical in git's standard merging state; the parent
//! (or user) resolves via `apply_patch` + [`merge_retry`], which stages
//! the resolved files and finalises the merge commit.
//!
//! Requires the canonical workspace to be a git repository. Non-git
//! workspaces are no longer supported — the diff/apply fallback has been
//! retired in favour of native `git merge`.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use tokio::process::Command;
use tokio::sync::Mutex;

use super::runtime_settings::{keys, Defaults, RuntimeSettings};

/// Resolve whether to prune on successful merge. Tool-handler convenience.
pub async fn resolve_prune_after_merge() -> bool {
    RuntimeSettings::get_or(
        keys::WORKSPACE_COPY_PRUNE_AFTER_MERGE,
        Defaults::WORKSPACE_COPY_PRUNE_AFTER_MERGE,
    )
    .await
}

/// Resolve the base directory for thread worktrees.
pub async fn resolve_base_dir() -> PathBuf {
    if let Ok(Some(custom)) = RuntimeSettings::load::<String>("workspace.copy.base_dir").await {
        let path = PathBuf::from(custom);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    default_base_dir()
}

fn default_base_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".hirsel/worktrees")
}

/// Per-canonical-workspace lock. Held while a merge is in-flight so two
/// concurrent `merge` calls against the same workspace serialise.
fn merge_locks() -> &'static StdMutex<Vec<(PathBuf, Arc<Mutex<()>>)>> {
    static MERGE_LOCKS: OnceLock<StdMutex<Vec<(PathBuf, Arc<Mutex<()>>)>>> = OnceLock::new();
    MERGE_LOCKS.get_or_init(|| StdMutex::new(Vec::new()))
}

fn lock_for(canonical: &Path) -> Arc<Mutex<()>> {
    let canonical = canonical.to_path_buf();
    let mut table = merge_locks().lock().expect("merge lock poisoned");
    if let Some((_, lock)) = table.iter().find(|(p, _)| p == &canonical) {
        return lock.clone();
    }
    let lock = Arc::new(Mutex::new(()));
    table.push((canonical, lock.clone()));
    lock
}

/// Branch name used for a thread's worktree.
fn branch_name(thread_id: &str) -> String {
    format!("hirsel-thread/{thread_id}")
}

/// Handle to a thread's worktree.
#[derive(Debug, Clone)]
pub struct WorkspaceCopy {
    pub thread_id: String,
    /// Absolute path to the canonical workspace (the user's original).
    pub canonical: PathBuf,
    /// Absolute path to the thread's worktree.
    pub copy_dir: PathBuf,
    /// Commit SHA at HEAD when the worktree was created.
    pub base_commit: Option<String>,
}

/// Create a git worktree for `thread_id` rooted at the canonical workspace.
/// Resolves `base_dir` from settings.
pub async fn create_copy(thread_id: &str, canonical: &Path) -> Result<WorkspaceCopy, String> {
    let base_dir = resolve_base_dir().await;
    create_copy_at(thread_id, canonical, &base_dir).await
}

/// Lower-level: create a worktree with an explicit base dir. Settings-free.
pub async fn create_copy_at(
    thread_id: &str,
    canonical: &Path,
    base_dir: &Path,
) -> Result<WorkspaceCopy, String> {
    if !canonical.is_dir() {
        return Err(format!(
            "workspace canonical path is not a directory: {}",
            canonical.display()
        ));
    }
    if !is_git_repo(canonical).await {
        return Err(format!(
            "worktree spawn requires a git repo at {}; run `git init` first",
            canonical.display()
        ));
    }

    tokio::fs::create_dir_all(base_dir)
        .await
        .map_err(|e| format!("create worktree base dir: {e}"))?;

    let copy_dir = base_dir.join(thread_id);
    let branch = branch_name(thread_id);

    // Clean up any stale state from a previous spawn with the same id.
    let _ = remove_worktree_if_present(canonical, &copy_dir).await;
    let _ = delete_branch_if_present(canonical, &branch).await;
    if copy_dir.exists() {
        tokio::fs::remove_dir_all(&copy_dir)
            .await
            .map_err(|e| format!("prune stale worktree dir: {e}"))?;
    }

    let base_commit = git_rev_parse_head(canonical).await.ok();

    // `git worktree add -b {branch} {path}` — creates the branch from HEAD
    // and checks it out into the target directory.
    let output = Command::new("git")
        .current_dir(canonical)
        .args(["worktree", "add", "-b", &branch])
        .arg(&copy_dir)
        .output()
        .await
        .map_err(|e| format!("git worktree add spawn: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(WorkspaceCopy {
        thread_id: thread_id.to_string(),
        canonical: canonical.to_path_buf(),
        copy_dir,
        base_commit,
    })
}

/// Remove the worktree and delete its branch. Safe to call on a worktree
/// that's already gone.
pub async fn discard(copy: &WorkspaceCopy) -> Result<(), String> {
    // Abort any pending merge in canonical first — if the user discarded
    // the source mid-conflict we don't want canonical stuck.
    if is_merge_in_progress(&copy.canonical).await {
        let _ = run_git(&copy.canonical, &["merge", "--abort"]).await;
    }
    let _ = remove_worktree_if_present(&copy.canonical, &copy.copy_dir).await;
    let _ = delete_branch_if_present(&copy.canonical, &branch_name(&copy.thread_id)).await;
    if copy.copy_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&copy.copy_dir).await;
    }
    Ok(())
}

/// Status of a single path in the merge diff.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileChange {
    pub path: String,
    /// One of: "added", "modified", "deleted", "renamed".
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    /// For renames, the old path. Otherwise `None`.
    pub from: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffStats {
    pub files: Vec<FileChange>,
    pub total_additions: u64,
    pub total_deletions: u64,
}

/// Inspect the working-tree delta between the thread's worktree and the
/// base commit the worktree was created from.
pub async fn inspect(copy: &WorkspaceCopy) -> Result<DiffStats, String> {
    let Some(base) = &copy.base_commit else {
        return Err("worktree has no base commit to diff against".to_string());
    };
    // `git add --intent-to-add` surfaces untracked files in diff output as
    // additions; harmless otherwise.
    let _ = run_git(&copy.copy_dir, &["add", "--intent-to-add", "--all", "."]).await;
    let files = git_numstat(&copy.copy_dir, base).await?;
    let total_additions = files.iter().map(|f| f.additions).sum();
    let total_deletions = files.iter().map(|f| f.deletions).sum();
    Ok(DiffStats {
        files,
        total_additions,
        total_deletions,
    })
}

/// Outcome of a merge attempt.
#[derive(Debug, Clone)]
pub enum MergeOutcome {
    Merged,
    Conflict { files: Vec<String> },
}

/// Merge via runtime-resolved settings. See [`merge_with`] for the
/// settings-free entry point.
pub async fn merge(copy: &WorkspaceCopy) -> Result<MergeOutcome, String> {
    let prune = resolve_prune_after_merge().await;
    merge_with(copy, prune).await
}

/// Merge the thread's worktree branch into the canonical workspace using
/// `git merge`. Serialises per-canonical-workspace via an in-process
/// mutex. `prune_on_success` is resolved by the caller — tests pass
/// `false` to keep the worktree for inspection.
///
/// If the thread has uncommitted working-tree changes, they are snapshot
/// into a single commit on the thread's branch first so `git merge`
/// actually sees them.
///
/// Conflicts leave canonical in git's standard merging state
/// (`MERGE_HEAD` present); the caller resolves with `apply_patch` and
/// calls [`merge_retry`] to finalise.
pub async fn merge_with(
    copy: &WorkspaceCopy,
    prune_on_success: bool,
) -> Result<MergeOutcome, String> {
    let lock = lock_for(&copy.canonical);
    let _guard = lock.lock().await;

    let branch = branch_name(&copy.thread_id);

    if is_merge_in_progress(&copy.canonical).await {
        return Err("a merge is already in progress in the canonical workspace".to_string());
    }

    snapshot_worktree_if_dirty(&copy.copy_dir, &copy.thread_id).await?;

    let output = run_git_raw(&copy.canonical, &["merge", "--no-ff", "--no-edit", &branch]).await?;

    finalise_merge_outcome(&copy.canonical, output, prune_on_success, copy).await
}

/// Retry a merge after the parent has written resolved file contents in
/// the canonical workspace.
///
/// If canonical is in an in-progress merge state we stage the resolved
/// files and complete the merge commit. Otherwise we fall through to a
/// fresh `merge`.
pub async fn merge_retry(copy: &WorkspaceCopy) -> Result<MergeOutcome, String> {
    let lock = lock_for(&copy.canonical);
    let _guard = lock.lock().await;

    if !is_merge_in_progress(&copy.canonical).await {
        drop(_guard);
        return merge(copy).await;
    }

    let _ = run_git(&copy.canonical, &["add", "--all"]).await;
    let unmerged = git_unmerged(&copy.canonical).await.unwrap_or_default();
    if !unmerged.is_empty() {
        return Ok(MergeOutcome::Conflict { files: unmerged });
    }

    // No conflicts left — finalise the merge commit.
    let output = run_git_raw(&copy.canonical, &["commit", "--no-edit"]).await?;
    if !output.status.success() {
        return Err(format!(
            "git commit (merge finalise) failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let prune = resolve_prune_after_merge().await;
    if prune {
        let _ = discard(copy).await;
    }
    Ok(MergeOutcome::Merged)
}

// ─────────────────────────────────────────────────────────────────────
// internals
// ─────────────────────────────────────────────────────────────────────

async fn is_git_repo(dir: &Path) -> bool {
    run_git(dir, &["rev-parse", "--git-dir"])
        .await
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

async fn is_merge_in_progress(dir: &Path) -> bool {
    let out = match Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "--verify", "MERGE_HEAD"])
        .output()
        .await
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    out.status.success()
}

async fn snapshot_worktree_if_dirty(worktree: &Path, thread_id: &str) -> Result<(), String> {
    let _ = run_git(worktree, &["add", "--all"]).await;
    // `status --porcelain` lines present iff the worktree (now staged) has
    // any changes to commit.
    let status = run_git(worktree, &["status", "--porcelain"])
        .await
        .unwrap_or_default();
    if status.trim().is_empty() {
        return Ok(());
    }
    let msg = format!("hirsel-thread/{thread_id}: snapshot");
    let output = run_git_raw(worktree, &["commit", "-m", &msg]).await?;
    if !output.status.success() {
        return Err(format!(
            "git commit (snapshot) failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

async fn finalise_merge_outcome(
    canonical: &Path,
    output: Output,
    prune_on_success: bool,
    copy: &WorkspaceCopy,
) -> Result<MergeOutcome, String> {
    if output.status.success() {
        if prune_on_success {
            let _ = discard(copy).await;
        }
        return Ok(MergeOutcome::Merged);
    }

    // Either conflict or a hard error. Inspect unmerged paths to decide.
    let unmerged = git_unmerged(canonical).await.unwrap_or_default();
    if !unmerged.is_empty() {
        return Ok(MergeOutcome::Conflict { files: unmerged });
    }

    Err(format!(
        "git merge failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

async fn remove_worktree_if_present(canonical: &Path, path: &Path) -> Result<(), String> {
    // `git worktree remove --force` is a no-op if the path isn't a
    // registered worktree, but it errors out. Run and ignore.
    let _ = Command::new("git")
        .current_dir(canonical)
        .args(["worktree", "remove", "--force"])
        .arg(path)
        .output()
        .await;
    let _ = Command::new("git")
        .current_dir(canonical)
        .args(["worktree", "prune"])
        .output()
        .await;
    Ok(())
}

async fn delete_branch_if_present(canonical: &Path, branch: &str) -> Result<(), String> {
    let _ = Command::new("git")
        .current_dir(canonical)
        .args(["branch", "-D", branch])
        .output()
        .await;
    Ok(())
}

async fn git_rev_parse_head(dir: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .await
        .map_err(|e| format!("git rev-parse spawn: {e}"))?;
    require_success(&output)?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn git_numstat(dir: &Path, base: &str) -> Result<Vec<FileChange>, String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["diff", "--numstat", base])
        .output()
        .await
        .map_err(|e| format!("git numstat spawn: {e}"))?;
    require_success(&output)?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let additions = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0u64);
        let deletions = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0u64);
        let path = parts.collect::<Vec<_>>().join(" ");
        if path.is_empty() {
            continue;
        }
        let status = if additions == 0 && deletions > 0 {
            "deleted"
        } else if deletions == 0 && additions > 0 {
            "added"
        } else {
            "modified"
        };
        out.push(FileChange {
            path,
            status: status.to_string(),
            additions,
            deletions,
            from: None,
        });
    }
    Ok(out)
}

async fn git_unmerged(dir: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["diff", "--name-only", "--diff-filter=U"])
        .output()
        .await
        .map_err(|e| format!("git unmerged spawn: {e}"))?;
    require_success(&output)?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

async fn run_git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = run_git_raw(dir, args).await?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn run_git_raw(dir: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("git {} spawn: {e}", args.first().copied().unwrap_or("?")))
}

fn require_success(output: &Output) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn init_git_dir() -> PathBuf {
        let dir = tempfile::tempdir().unwrap().keep();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .current_dir(&dir)
                .args(args)
                .output()
                .unwrap();
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.join("README.md"), "hello\n").unwrap();
        run(&["add", "README.md"]);
        run(&["commit", "-qm", "init"]);
        dir
    }

    async fn test_base_dir() -> PathBuf {
        tempfile::tempdir().unwrap().keep()
    }

    #[tokio::test]
    async fn worktree_create_merge_roundtrip() {
        let source = init_git_dir().await;
        let base = test_base_dir().await;
        let copy = create_copy_at("thread-A", &source, &base)
            .await
            .expect("create");
        assert!(copy.copy_dir.exists());
        assert!(copy.base_commit.is_some());

        // Make changes inside the worktree.
        std::fs::write(copy.copy_dir.join("NEW.md"), "from thread\n").unwrap();
        std::fs::write(copy.copy_dir.join("README.md"), "hello world\n").unwrap();

        let stats = inspect(&copy).await.expect("inspect");
        assert!(stats.files.iter().any(|f| f.path == "NEW.md"));
        assert!(stats.files.iter().any(|f| f.path == "README.md"));

        // Merge back into source (don't prune so we can keep asserting).
        let result = merge_with(&copy, false).await.expect("merge");
        assert!(matches!(result, MergeOutcome::Merged));

        let readme = std::fs::read_to_string(source.join("README.md")).unwrap();
        assert_eq!(readme, "hello world\n");
        let new_file = std::fs::read_to_string(source.join("NEW.md")).unwrap();
        assert_eq!(new_file, "from thread\n");
    }

    #[tokio::test]
    async fn merge_conflict_then_retry() {
        let source = init_git_dir().await;
        let base = test_base_dir().await;
        let copy = create_copy_at("thread-B", &source, &base)
            .await
            .expect("create");

        // Diverge: canonical edits README differently, then so does the
        // worktree — same file, same area, guaranteed conflict.
        std::fs::write(source.join("README.md"), "canonical change\n").unwrap();
        std::process::Command::new("git")
            .current_dir(&source)
            .args(["commit", "-qam", "canon"])
            .output()
            .unwrap();

        std::fs::write(copy.copy_dir.join("README.md"), "thread change\n").unwrap();

        let result = merge_with(&copy, false).await.expect("merge");
        let files = match result {
            MergeOutcome::Conflict { files } => files,
            _ => panic!("expected conflict"),
        };
        assert!(files.iter().any(|f| f.ends_with("README.md")));
        assert!(is_merge_in_progress(&source).await);

        // Parent resolves the conflict by writing a merged version.
        std::fs::write(source.join("README.md"), "resolved content\n").unwrap();
        let finalised = merge_retry(&copy).await.expect("retry");
        assert!(matches!(finalised, MergeOutcome::Merged));
        assert!(!is_merge_in_progress(&source).await);

        let readme = std::fs::read_to_string(source.join("README.md")).unwrap();
        assert_eq!(readme, "resolved content\n");
    }

    #[tokio::test]
    async fn discard_cleans_up_worktree_and_branch() {
        let source = init_git_dir().await;
        let base = test_base_dir().await;
        let copy = create_copy_at("thread-C", &source, &base)
            .await
            .expect("create");
        assert!(copy.copy_dir.exists());

        discard(&copy).await.expect("discard");
        assert!(!copy.copy_dir.exists());

        // Branch is gone too.
        let branches = run_git(&source, &["branch", "--list", "hirsel-thread/thread-C"])
            .await
            .unwrap();
        assert!(branches.trim().is_empty(), "branch should be deleted");
    }
}
