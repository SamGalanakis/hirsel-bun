//! Per-thread workspace copies and merge helpers.
//!
//! A thread with `workspace_write` in its caps gets a full CoW-reflinked
//! copy of the canonical workspace at its own directory. The copy is
//! self-contained: tools, shells, editors, and reviewers can all operate
//! against it without touching the canonical state.
//!
//! Merging writes the copy's working-tree delta back onto the canonical
//! workspace using git's 3-way merge (when the workspace is a git repo).
//! Conflicts are surfaced structurally so the parent thread (or user) can
//! resolve them in-place via `apply_patch` + `merge_retry`.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use tokio::process::Command;
use tokio::sync::Mutex;

use super::runtime_settings::{keys, Defaults, RuntimeSettings};

/// Controls the CoW behaviour for a copy. Callers resolve the effective
/// value (from settings or overrides) and pass it in; this module stays
/// pure so unit tests don't need the global DB.
#[derive(Debug, Clone, Copy, Default)]
pub enum CowMode {
    #[default]
    /// Use `cp --reflink=auto` (reflink if the FS supports it, else copy).
    Auto,
    /// Force `cp --reflink=always` — error if unsupported.
    On,
    /// Force `cp --reflink=never`.
    Off,
}

impl CowMode {
    fn flag(self) -> &'static str {
        match self {
            CowMode::Auto => "--reflink=auto",
            CowMode::On => "--reflink=always",
            CowMode::Off => "--reflink=never",
        }
    }

    pub fn from_setting_value(value: &str) -> Self {
        match value {
            "on" => CowMode::On,
            "off" => CowMode::Off,
            _ => CowMode::Auto,
        }
    }
}

/// Resolve the CoW mode from runtime settings. Intended for tool handlers;
/// tests should pass an explicit `CowMode` to `create_copy_with_mode`.
pub async fn resolve_cow_mode() -> CowMode {
    let value = RuntimeSettings::get_or(
        keys::WORKSPACE_COPY_COW_MODE,
        Defaults::WORKSPACE_COPY_COW_MODE.to_string(),
    )
    .await;
    CowMode::from_setting_value(&value)
}

/// Resolve whether to prune on successful merge. Tool-handler convenience.
pub async fn resolve_prune_after_merge() -> bool {
    RuntimeSettings::get_or(
        keys::WORKSPACE_COPY_PRUNE_AFTER_MERGE,
        Defaults::WORKSPACE_COPY_PRUNE_AFTER_MERGE,
    )
    .await
}

/// Resolve the base directory for thread workspace copies.
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
        .join(".hirsel/workspaces")
}

/// Per-canonical-workspace lock. Held while a merge is in-flight so two
/// concurrent `merge_thread` calls against the same workspace serialise.
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

/// Handle to a thread's workspace copy. Cheap to clone; the filesystem does
/// the heavy lifting.
#[derive(Debug, Clone)]
pub struct WorkspaceCopy {
    pub thread_id: String,
    /// Absolute path to the canonical workspace (the user's original).
    pub canonical: PathBuf,
    /// Absolute path to the thread's CoW copy.
    pub copy_dir: PathBuf,
    /// Commit SHA at HEAD when the copy was taken (None for non-git
    /// workspaces).
    pub base_commit: Option<String>,
}

/// Create a CoW copy of `canonical` for the given thread. Convenience
/// wrapper that resolves CoW mode + base dir from runtime settings.
///
/// Layout: `{base_dir}/{thread_id}/`. Uses `cp --reflink=auto` so on
/// btrfs/apfs/xfs/zfs the copy is near-instant and shares storage until
/// divergence.
pub async fn create_copy(thread_id: &str, canonical: &Path) -> Result<WorkspaceCopy, String> {
    let base_dir = resolve_base_dir().await;
    let mode = resolve_cow_mode().await;
    create_copy_with_mode(thread_id, canonical, &base_dir, mode).await
}

/// Lower-level: make a copy with explicit base dir + mode. Settings-free,
/// suitable for unit tests.
pub async fn create_copy_with_mode(
    thread_id: &str,
    canonical: &Path,
    base_dir: &Path,
    mode: CowMode,
) -> Result<WorkspaceCopy, String> {
    if !canonical.is_dir() {
        return Err(format!(
            "workspace canonical path is not a directory: {}",
            canonical.display()
        ));
    }
    tokio::fs::create_dir_all(base_dir)
        .await
        .map_err(|e| format!("create workspace copy root: {e}"))?;

    let copy_dir = base_dir.join(thread_id);
    if copy_dir.exists() {
        tokio::fs::remove_dir_all(&copy_dir)
            .await
            .map_err(|e| format!("prune stale copy dir: {e}"))?;
    }

    let canonical_arg = {
        let mut s = canonical.as_os_str().to_os_string();
        s.push("/.");
        s
    };

    let output = Command::new("cp")
        .arg("-a")
        .arg(mode.flag())
        .arg(&canonical_arg)
        .arg(&copy_dir)
        .output()
        .await
        .map_err(|e| format!("cp failed to start: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cp failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let base_commit = git_rev_parse_head(&copy_dir).await.ok();

    Ok(WorkspaceCopy {
        thread_id: thread_id.to_string(),
        canonical: canonical.to_path_buf(),
        copy_dir,
        base_commit,
    })
}

/// Delete the copy directory. Safe to call on a copy that's already gone.
pub async fn discard(copy: &WorkspaceCopy) -> Result<(), String> {
    if copy.copy_dir.exists() {
        tokio::fs::remove_dir_all(&copy.copy_dir)
            .await
            .map_err(|e| format!("discard workspace copy: {e}"))?;
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

/// Inspect the working-tree delta between the thread's copy and the base
/// commit it was taken from.
pub async fn inspect(copy: &WorkspaceCopy) -> Result<DiffStats, String> {
    if copy.base_commit.is_none() {
        return Err("workspace is not a git repo; diff/merge requires git".to_string());
    }
    git_stage_intent_to_add(&copy.copy_dir).await?;
    let files = git_numstat(&copy.copy_dir, "HEAD").await?;
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

/// Merge the thread's workspace-copy delta back onto the canonical
/// workspace using git's 3-way merge.
///
/// Serialises per-canonical-workspace via an in-process mutex. `prune_on_success`
/// is resolved by the caller — tests pass `false` to keep the copy dir for
/// inspection.
pub async fn merge_with(
    copy: &WorkspaceCopy,
    prune_on_success: bool,
) -> Result<MergeOutcome, String> {
    let Some(base_commit) = copy.base_commit.clone() else {
        return Err("workspace is not a git repo; diff/merge requires git".to_string());
    };

    let lock = lock_for(&copy.canonical);
    let _guard = lock.lock().await;

    // Include any untracked files as additions in the diff.
    git_stage_intent_to_add(&copy.copy_dir).await?;

    // Build a patch from the copy's working tree vs its base commit.
    let patch = git_diff_text(&copy.copy_dir, &base_commit).await?;
    if patch.trim().is_empty() {
        return Ok(MergeOutcome::Merged);
    }

    // Apply with 3-way merge on the canonical side so local advances are
    // reconciled.
    let apply = Command::new("git")
        .current_dir(&copy.canonical)
        .args(["apply", "--3way", "--index"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("git apply spawn: {e}"))?;

    let apply_with_stdin = async move {
        let mut child = apply;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(patch.as_bytes())
                .await
                .map_err(|e| format!("write patch to git apply stdin: {e}"))?;
        }
        child
            .wait_with_output()
            .await
            .map_err(|e| format!("git apply wait: {e}"))
    };
    let output = apply_with_stdin.await?;

    if output.status.success() {
        if prune_on_success {
            let _ = discard(copy).await;
        }
        return Ok(MergeOutcome::Merged);
    }

    // Parse conflict file list from stderr. `git apply --3way` prints lines
    // like "Applied patch to 'foo.rs' with conflicts." and "error:
    // conflicts found in foo.rs".
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut files: Vec<String> = Vec::new();
    for line in stderr.lines() {
        if let Some(rest) = line.strip_prefix("U\t") {
            files.push(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("Applied patch to '") {
            if let Some(end) = rest.find('\'') {
                files.push(rest[..end].to_string());
            }
        }
    }
    // Fallback: ask git for unmerged paths.
    if files.is_empty() {
        if let Ok(unmerged) = git_unmerged(&copy.canonical).await {
            files = unmerged;
        }
    }
    if files.is_empty() {
        return Err(format!("git apply failed: {}", stderr.trim()));
    }
    Ok(MergeOutcome::Conflict { files })
}

/// Retry `merge` after the parent has written resolved file contents in the
/// canonical workspace.
pub async fn merge_retry(copy: &WorkspaceCopy) -> Result<MergeOutcome, String> {
    merge(copy).await
}

// ─────────────────────────────────────────────────────────────────────
// internals
// ─────────────────────────────────────────────────────────────────────

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

/// `git add --intent-to-add .` — makes untracked files visible to `git diff`
/// as additions without committing their content.
async fn git_stage_intent_to_add(dir: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["add", "--intent-to-add", "--all", "."])
        .output()
        .await
        .map_err(|e| format!("git add -N spawn: {e}"))?;
    require_success(&output)
}

async fn git_diff_text(dir: &Path, base: &str) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["diff", "--binary", base])
        .output()
        .await
        .map_err(|e| format!("git diff spawn: {e}"))?;
    require_success(&output)?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.join("README.md"), "hello\n").unwrap();
        run(&["add", "README.md"]);
        run(&["commit", "-qm", "init"]);
        dir
    }

    async fn test_copy_dir() -> PathBuf {
        tempfile::tempdir().unwrap().keep()
    }

    #[tokio::test]
    async fn cow_copy_and_merge_roundtrip() {
        let source = init_git_dir().await;
        let base = test_copy_dir().await;
        let copy = create_copy_with_mode("thread-A", &source, &base, CowMode::Auto)
            .await
            .expect("create");
        assert!(copy.copy_dir.exists());
        assert!(copy.base_commit.is_some());

        // Write a change in the copy; verify inspect() reports it.
        std::fs::write(copy.copy_dir.join("NEW.md"), "from thread\n").unwrap();
        std::fs::write(copy.copy_dir.join("README.md"), "hello world\n").unwrap();

        let stats = inspect(&copy).await.expect("inspect");
        assert!(stats.files.iter().any(|f| f.path == "NEW.md"));
        assert!(stats.files.iter().any(|f| f.path == "README.md"));

        // Merge back into source without auto-pruning, so we can assert.
        let result = merge_with(&copy, false).await.expect("merge");
        assert!(matches!(result, MergeOutcome::Merged));

        // Canonical should now contain the thread's work.
        let readme = std::fs::read_to_string(source.join("README.md")).unwrap();
        assert_eq!(readme, "hello world\n");
        let new_file = std::fs::read_to_string(source.join("NEW.md")).unwrap();
        assert_eq!(new_file, "from thread\n");
    }

    #[tokio::test]
    async fn merge_conflict_is_surfaced_structurally() {
        let source = init_git_dir().await;
        let base = test_copy_dir().await;
        let copy = create_copy_with_mode("thread-B", &source, &base, CowMode::Auto)
            .await
            .expect("create");

        // Same line edited differently on both sides — guaranteed conflict.
        std::fs::write(source.join("README.md"), "canonical change\n").unwrap();
        std::process::Command::new("git")
            .current_dir(&source)
            .args(["commit", "-qam", "canon"])
            .output()
            .unwrap();

        std::fs::write(copy.copy_dir.join("README.md"), "thread change\n").unwrap();

        let result = merge_with(&copy, false).await.expect("merge");
        match result {
            MergeOutcome::Conflict { files } => {
                assert!(files.iter().any(|f| f.ends_with("README.md")));
            }
            MergeOutcome::Merged => panic!("expected conflict"),
        }
    }
}
