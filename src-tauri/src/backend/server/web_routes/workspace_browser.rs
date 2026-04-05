use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path as FsPath, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use axum::body::Body;
use axum::extract::{Multipart, Path, Query};
use axum::http::header::{self, HeaderMap, HeaderValue};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use imara_diff::{Algorithm, Diff, InternedInput};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::backend::draft::StartingPoint;
use crate::backend::git::get_current_branch;
use crate::backend::{
    ensure_project_workspace, ensure_thread_checkout, ProjectWorkspace, ShepherdThreadStore,
};

const DEFAULT_FILE_SLICE_LINES: usize = 160;
const MAX_FILE_SLICE_LINES: usize = 600;
const MAX_TEXT_PREVIEW_BYTES: usize = 2_000_000;
const TEXT_SNIFF_BYTES: usize = 8_192;
const MAX_SEARCH_RESULTS: usize = 250;
const WORKSPACE_INDEX_TTL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWorkspaceRoot {
    id: String,
    kind: String,
    label: String,
    status: String,
    summary: Option<String>,
    thread_id: Option<String>,
    branch: Option<String>,
    read_only: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWorkspaceTreeEntry {
    root_id: String,
    path: String,
    name: String,
    kind: String,
    size: Option<u64>,
    modified_at: Option<String>,
    mime: Option<String>,
    is_text: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWorkspaceTree {
    root_id: String,
    path: String,
    entries: Vec<ApiWorkspaceTreeEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWorkspaceFile {
    root_id: String,
    path: String,
    name: String,
    size: u64,
    modified_at: Option<String>,
    mime: Option<String>,
    is_text: bool,
    line_start: usize,
    line_end: usize,
    total_lines: usize,
    truncated: bool,
    content: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWorkspaceSearchResult {
    root_id: String,
    path: String,
    line: usize,
    column: usize,
    preview: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWorkspaceSearchResponse {
    query: String,
    results: Vec<ApiWorkspaceSearchResult>,
    truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWorkspaceCompletionEntry {
    path: String,
    kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWorkspaceDiffEntry {
    path: String,
    status: String,
    is_text: bool,
    left_mime: Option<String>,
    right_mime: Option<String>,
    left_size: Option<u64>,
    right_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWorkspaceDiffSummary {
    left_root_id: String,
    right_root_id: String,
    entries: Vec<ApiWorkspaceDiffEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWorkspaceDiffFileSide {
    root_id: String,
    path: String,
    name: String,
    size: u64,
    modified_at: Option<String>,
    mime: Option<String>,
    is_text: bool,
    truncated: bool,
    content: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiWorkspaceDiffFile {
    left_root_id: String,
    right_root_id: String,
    path: String,
    status: String,
    is_text: bool,
    additions: Option<u32>,
    deletions: Option<u32>,
    left: Option<ApiWorkspaceDiffFileSide>,
    right: Option<ApiWorkspaceDiffFileSide>,
}

#[derive(Debug, Clone)]
struct WorkspaceIndexedFile {
    absolute_path: PathBuf,
    size: u64,
    mime: Option<String>,
    is_text: bool,
}

#[derive(Debug, Clone)]
struct WorkspaceTreeIndexEntry {
    path: String,
    name: String,
    kind: String,
    size: Option<u64>,
    modified_at: Option<String>,
    mime: Option<String>,
    is_text: Option<bool>,
}

#[derive(Debug, Clone)]
struct WorkspaceRootIndex {
    directories: BTreeMap<String, Vec<WorkspaceTreeIndexEntry>>,
    files: BTreeMap<String, WorkspaceIndexedFile>,
}

#[derive(Debug, Clone)]
struct CachedWorkspaceIndex {
    built_at: Instant,
    index: Arc<WorkspaceRootIndex>,
}

static WORKSPACE_INDEX_CACHE: OnceLock<StdMutex<HashMap<String, CachedWorkspaceIndex>>> =
    OnceLock::new();

#[derive(Debug)]
enum WorkspaceRootRef {
    Main,
    Thread(String),
    Remote,
}

#[derive(Deserialize)]
pub struct WorkspaceTreeQuery {
    root_id: String,
    path: Option<String>,
}

#[derive(Deserialize)]
pub struct WorkspaceFileQuery {
    root_id: String,
    path: String,
    line_start: Option<usize>,
    line_end: Option<usize>,
}

#[derive(Deserialize)]
pub struct WorkspaceSearchQuery {
    q: String,
    root_id: Option<String>,
}

#[derive(Deserialize)]
pub struct WorkspaceCompleteQuery {
    root_id: String,
    prefix: Option<String>,
}

#[derive(Deserialize)]
pub struct WorkspaceDiffQuery {
    left_root_id: String,
    right_root_id: String,
}

#[derive(Deserialize)]
pub struct WorkspaceDiffFileQuery {
    left_root_id: String,
    right_root_id: String,
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkspaceFileBody {
    root_id: String,
    path: String,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceWriteResponse {
    ok: bool,
    root_id: String,
    path: String,
}

fn parse_root_id(raw: &str) -> Result<WorkspaceRootRef, String> {
    let value = raw.trim();
    if value.eq_ignore_ascii_case("main") {
        return Ok(WorkspaceRootRef::Main);
    }
    if let Some(thread_id) = value.strip_prefix("thread:") {
        let thread_id = thread_id.trim();
        if thread_id.is_empty() {
            return Err("thread root_id is missing a thread id".to_string());
        }
        return Ok(WorkspaceRootRef::Thread(thread_id.to_string()));
    }
    if value.eq_ignore_ascii_case("remote") {
        return Ok(WorkspaceRootRef::Remote);
    }
    Err(format!("unknown workspace root_id '{}'", value))
}

async fn resolve_root_path(project_id: i64, root_id: &str) -> Result<PathBuf, String> {
    match parse_root_id(root_id)? {
        WorkspaceRootRef::Main => Ok(ensure_project_workspace(project_id).await?.central_dir),
        WorkspaceRootRef::Remote => ensure_remote_worktree(project_id).await,
        WorkspaceRootRef::Thread(thread_id) => {
            let (path, _) = ensure_thread_checkout(project_id, &thread_id).await?;
            let root = PathBuf::from(path);
            if !root.exists() || !root.is_dir() {
                return Err(format!(
                    "workspace root '{}' points at a missing checkout",
                    root_id
                ));
            }
            Ok(root)
        }
    }
}

/// Ensure a read-only git worktree of `origin/{branch}` exists for browsing
/// the upstream state. Lives at `~/.hirsel/workspaces/<project-workspace>/work/remote/`.
async fn ensure_remote_worktree(project_id: i64) -> Result<PathBuf, String> {
    let ws = ensure_project_workspace(project_id).await?;
    let branch = resolve_project_remote_branch(&ws)
        .ok_or_else(|| "project has no browsable origin branch".to_string())?;
    let remote_ref = format!("origin/{}", branch);
    let remote_dir = ws.workspace_dir.join("work").join("remote");

    // Fetch latest from origin (best-effort, don't fail if offline)
    let _ = std::process::Command::new("git")
        .args(["fetch", "origin", &branch, "--quiet"])
        .current_dir(&ws.central_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if remote_dir.join(".git").exists() {
        // Worktree exists — update it to match origin
        let _ = std::process::Command::new("git")
            .args(["checkout", "--detach", &remote_ref, "--quiet"])
            .current_dir(&remote_dir)
            .output();
        return Ok(remote_dir);
    }

    // Create new worktree
    let output = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "--detach",
            &remote_dir.display().to_string(),
            &remote_ref,
        ])
        .current_dir(&ws.central_dir)
        .output()
        .map_err(|e| format!("failed to create remote worktree: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(remote_dir)
}

fn git_command_success(repo_dir: &FsPath, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(repo_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn git_origin_head_branch(repo_dir: &FsPath) -> Option<String> {
    let output = Command::new("git")
        .args([
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ])
        .current_dir(repo_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    value.strip_prefix("origin/").map(ToOwned::to_owned)
}

fn git_has_origin_ref(repo_dir: &FsPath, branch: &str) -> bool {
    git_command_success(
        repo_dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("origin/{}", branch),
        ],
    ) || git_command_success(
        repo_dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{}", branch),
        ],
    )
}

fn resolve_project_remote_branch(workspace: &ProjectWorkspace) -> Option<String> {
    if !git_command_success(&workspace.central_dir, &["remote", "get-url", "origin"]) {
        return None;
    }

    let mut candidates = Vec::new();

    if let StartingPoint::GitRepo {
        branch: Some(branch),
        ..
    } = &workspace.project.starting_point
    {
        let trimmed = branch.trim();
        if !trimmed.is_empty() {
            candidates.push(trimmed.to_string());
        }
    }

    if let Ok(branch) = get_current_branch(&workspace.central_dir) {
        if !branch.trim().is_empty() && branch != "central" {
            candidates.push(branch);
        }
    }

    if let Some(branch) = git_origin_head_branch(&workspace.central_dir) {
        if !branch.trim().is_empty() {
            candidates.push(branch);
        }
    }

    candidates.dedup();

    for branch in candidates {
        if git_has_origin_ref(&workspace.central_dir, &branch) {
            return Some(branch);
        }
    }

    match &workspace.project.starting_point {
        StartingPoint::GitRepo {
            branch: Some(branch),
            ..
        } => Some(branch.trim().to_string()),
        _ => git_origin_head_branch(&workspace.central_dir),
    }
}

fn resolve_workspace_path(root: &FsPath, relative: &str) -> Result<PathBuf, String> {
    let trimmed = relative.trim();
    if trimmed.is_empty() {
        return Ok(root.to_path_buf());
    }

    let path = FsPath::new(trimmed);
    if path.is_absolute() {
        return Err("workspace paths must be relative".to_string());
    }

    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            _ => {
                return Err("workspace paths may not escape the selected workspace".to_string());
            }
        }
    }

    Ok(root.join(path))
}

fn relative_path(root: &FsPath, path: &FsPath) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn workspace_index_cache() -> &'static StdMutex<HashMap<String, CachedWorkspaceIndex>> {
    WORKSPACE_INDEX_CACHE.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn workspace_cache_key(root: &FsPath) -> String {
    root.to_string_lossy().to_string()
}

fn build_tree_index_entry(
    root: &FsPath,
    path: &FsPath,
    metadata: &std::fs::Metadata,
) -> WorkspaceTreeIndexEntry {
    let mime = metadata.is_file().then(|| guess_mime(path)).flatten();
    let is_text = metadata
        .is_file()
        .then(|| guess_text_from_mime(mime.as_deref()))
        .flatten();
    WorkspaceTreeIndexEntry {
        path: relative_path(root, path),
        name: basename(path),
        kind: if metadata.is_dir() {
            "directory".to_string()
        } else {
            "file".to_string()
        },
        size: metadata.is_file().then_some(metadata.len()),
        modified_at: metadata.modified().ok().map(system_time_to_iso),
        mime,
        is_text,
    }
}

fn build_workspace_root_index(root: &FsPath) -> Result<WorkspaceRootIndex, String> {
    let mut directories: BTreeMap<String, Vec<WorkspaceTreeIndexEntry>> = BTreeMap::new();
    let mut files = BTreeMap::new();
    directories.entry(String::new()).or_default();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != OsStr::new(".git"))
    {
        let entry =
            entry.map_err(|error| format!("failed to walk '{}': {}", root.display(), error))?;
        let path = entry.path();
        if path == root {
            continue;
        }

        let metadata = entry
            .metadata()
            .map_err(|error| format!("failed to stat '{}': {}", path.display(), error))?;
        let relative = relative_path(root, path);
        let parent = FsPath::new(&relative)
            .parent()
            .map(|value| value.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let tree_entry = build_tree_index_entry(root, path, &metadata);
        directories.entry(parent).or_default().push(tree_entry);

        if metadata.is_dir() {
            directories.entry(relative).or_default();
            continue;
        }

        let mime = guess_mime(path);
        let is_text = detect_text_from_path(path, mime.as_deref())?;
        files.insert(
            relative,
            WorkspaceIndexedFile {
                absolute_path: path.to_path_buf(),
                size: metadata.len(),
                mime,
                is_text,
            },
        );
    }

    for entries in directories.values_mut() {
        entries.sort_by(|left, right| {
            (if left.kind == "directory" { 0 } else { 1 })
                .cmp(&(if right.kind == "directory" { 0 } else { 1 }))
                .then_with(|| {
                    left.name
                        .to_ascii_lowercase()
                        .cmp(&right.name.to_ascii_lowercase())
                })
        });
    }

    Ok(WorkspaceRootIndex { directories, files })
}

fn workspace_root_index(root: &FsPath) -> Result<Arc<WorkspaceRootIndex>, String> {
    let key = workspace_cache_key(root);
    if let Some(cached) = workspace_index_cache()
        .lock()
        .map_err(|_| "workspace index cache lock poisoned".to_string())?
        .get(&key)
        .cloned()
        .filter(|cached| cached.built_at.elapsed() < WORKSPACE_INDEX_TTL)
    {
        return Ok(cached.index);
    }

    let index = Arc::new(build_workspace_root_index(root)?);
    workspace_index_cache()
        .lock()
        .map_err(|_| "workspace index cache lock poisoned".to_string())?
        .insert(
            key,
            CachedWorkspaceIndex {
                built_at: Instant::now(),
                index: Arc::clone(&index),
            },
        );
    Ok(index)
}

fn invalidate_workspace_index(root: &FsPath) {
    if let Ok(mut cache) = workspace_index_cache().lock() {
        cache.remove(&workspace_cache_key(root));
    }
}

fn guess_mime(path: &FsPath) -> Option<String> {
    mime_guess::from_path(path).first_raw().map(str::to_string)
}

fn guess_text_from_mime(mime: Option<&str>) -> Option<bool> {
    let mime = mime?;
    if mime.starts_with("text/") {
        return Some(true);
    }
    let text_like = [
        "application/json",
        "application/ld+json",
        "application/xml",
        "application/yaml",
        "application/x-yaml",
        "application/javascript",
        "application/x-javascript",
        "application/typescript",
        "application/x-sh",
        "application/x-shellscript",
        "image/svg+xml",
    ];
    Some(text_like.contains(&mime))
}

fn system_time_to_iso(value: SystemTime) -> String {
    DateTime::<Utc>::from(value).to_rfc3339()
}

fn detect_text(bytes: &[u8], mime: Option<&str>) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    if let Some(from_mime) = guess_text_from_mime(mime) {
        if from_mime {
            return std::str::from_utf8(bytes).is_ok();
        }
    }
    std::str::from_utf8(bytes).is_ok()
}

fn detect_text_from_path(path: &FsPath, mime: Option<&str>) -> Result<bool, String> {
    if let Some(from_mime) = guess_text_from_mime(mime) {
        return Ok(from_mime);
    }

    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("failed to open '{}': {}", path.display(), error))?;
    let mut buffer = vec![0; TEXT_SNIFF_BYTES];
    let read = file
        .read(&mut buffer)
        .map_err(|error| format!("failed to read '{}': {}", path.display(), error))?;
    buffer.truncate(read);
    Ok(detect_text(&buffer, mime))
}

fn resolve_line_bounds(line_start: Option<usize>, line_end: Option<usize>) -> (usize, usize, bool) {
    let start = line_start.unwrap_or(1).max(1);
    let requested_end =
        line_end.unwrap_or(start.saturating_add(DEFAULT_FILE_SLICE_LINES.saturating_sub(1)));
    let capped_end =
        requested_end.min(start.saturating_add(MAX_FILE_SLICE_LINES.saturating_sub(1)));
    (start, capped_end, capped_end < requested_end)
}

fn read_text_preview_slice(
    target: &FsPath,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<(String, usize, usize, usize, bool), String> {
    let file = std::fs::File::open(target)
        .map_err(|error| format!("failed to open '{}': {}", target.display(), error))?;
    let reader = BufReader::new(file);
    let (start, end, mut truncated) = resolve_line_bounds(line_start, line_end);
    let mut total_lines = 0usize;
    let mut selected = Vec::new();

    for line in reader.lines() {
        let line =
            line.map_err(|error| format!("failed to read '{}': {}", target.display(), error))?;
        total_lines += 1;
        if total_lines < start {
            continue;
        }
        if total_lines <= end {
            selected.push(line);
            continue;
        }
        truncated = true;
    }

    let actual_end = if selected.is_empty() {
        start.saturating_sub(1)
    } else {
        start + selected.len() - 1
    };
    Ok((
        selected.join("\n"),
        start,
        actual_end,
        total_lines,
        truncated,
    ))
}

fn basename(path: &FsPath) -> String {
    path.file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_string()
}

fn sanitize_upload_name(name: &str) -> Result<String, String> {
    let normalized = name.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    if normalized.is_empty() {
        return Err("uploaded file is missing a usable filename".to_string());
    }
    if normalized == "." || normalized == ".." {
        return Err("uploaded filename is invalid".to_string());
    }
    if normalized.contains('\0') {
        return Err("uploaded filename contains invalid bytes".to_string());
    }
    Ok(normalized.to_string())
}

fn complete_workspace_path(
    root: &FsPath,
    prefix: &str,
) -> Result<Vec<ApiWorkspaceCompletionEntry>, String> {
    let trimmed = prefix.trim();
    let (dir, dir_prefix, name_prefix) = if trimmed.is_empty() {
        (root.to_path_buf(), String::new(), String::new())
    } else if trimmed.ends_with('/') {
        let dir = resolve_workspace_path(root, trimmed)?;
        (dir, trimmed.to_string(), String::new())
    } else {
        let path = FsPath::new(trimmed);
        let parent = path
            .parent()
            .and_then(|value| {
                let relative = value.to_string_lossy().replace('\\', "/");
                if relative.is_empty() {
                    Some(root.to_path_buf())
                } else {
                    resolve_workspace_path(root, &relative).ok()
                }
            })
            .unwrap_or_else(|| root.to_path_buf());
        let dir_prefix = path
            .parent()
            .map(|value| {
                let relative = value.to_string_lossy().replace('\\', "/");
                if relative.is_empty() {
                    String::new()
                } else {
                    format!("{}/", relative.trim_end_matches('/'))
                }
            })
            .unwrap_or_default();
        let name_prefix = path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();
        (parent, dir_prefix, name_prefix)
    };

    let entries = std::fs::read_dir(&dir)
        .map_err(|error| format!("failed to read '{}': {}", dir.display(), error))?;
    let show_hidden = name_prefix.starts_with('.');
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        if !name_prefix.is_empty() && !name.starts_with(&name_prefix) {
            continue;
        }
        let is_dir = entry
            .file_type()
            .map(|value| value.is_dir())
            .unwrap_or(false);
        let relative = if is_dir {
            format!("{}{}/", dir_prefix, name)
        } else {
            format!("{}{}", dir_prefix, name)
        };
        let item = ApiWorkspaceCompletionEntry {
            path: relative,
            kind: if is_dir { "directory" } else { "file" }.to_string(),
        };
        if is_dir {
            dirs.push(item);
        } else {
            files.push(item);
        }
    }

    dirs.sort_by(|left, right| left.path.cmp(&right.path));
    files.sort_by(|left, right| left.path.cmp(&right.path));
    dirs.extend(files);
    dirs.truncate(20);
    Ok(dirs)
}

fn build_download_headers(path: &FsPath, mime: Option<&str>) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    let mime_value = mime.unwrap_or("application/octet-stream");
    let content_type = HeaderValue::from_str(mime_value)
        .map_err(|error| format!("invalid content type '{}': {}", mime_value, error))?;
    headers.insert(header::CONTENT_TYPE, content_type);

    let filename = basename(path).replace('"', "_");
    let disposition = HeaderValue::from_str(&format!("inline; filename=\"{}\"", filename))
        .map_err(|error| format!("invalid content disposition: {}", error))?;
    headers.insert(header::CONTENT_DISPOSITION, disposition);
    Ok(headers)
}

fn to_api_tree_entry(root_id: &str, entry: &WorkspaceTreeIndexEntry) -> ApiWorkspaceTreeEntry {
    ApiWorkspaceTreeEntry {
        root_id: root_id.to_string(),
        path: entry.path.clone(),
        name: entry.name.clone(),
        kind: entry.kind.clone(),
        size: entry.size,
        modified_at: entry.modified_at.clone(),
        mime: entry.mime.clone(),
        is_text: entry.is_text,
    }
}

fn index_workspace_files(root: &FsPath) -> Result<BTreeMap<String, WorkspaceIndexedFile>, String> {
    Ok(workspace_root_index(root)?.files.clone())
}

fn files_identical(left: &FsPath, right: &FsPath) -> Result<bool, String> {
    let left_meta = std::fs::metadata(left)
        .map_err(|error| format!("failed to stat '{}': {}", left.display(), error))?;
    let right_meta = std::fs::metadata(right)
        .map_err(|error| format!("failed to stat '{}': {}", right.display(), error))?;
    if left_meta.len() != right_meta.len() {
        return Ok(false);
    }

    let mut left_file = std::fs::File::open(left)
        .map_err(|error| format!("failed to open '{}': {}", left.display(), error))?;
    let mut right_file = std::fs::File::open(right)
        .map_err(|error| format!("failed to open '{}': {}", right.display(), error))?;
    let mut left_buffer = [0u8; 16_384];
    let mut right_buffer = [0u8; 16_384];

    loop {
        let left_read = left_file
            .read(&mut left_buffer)
            .map_err(|error| format!("failed to read '{}': {}", left.display(), error))?;
        let right_read = right_file
            .read(&mut right_buffer)
            .map_err(|error| format!("failed to read '{}': {}", right.display(), error))?;
        if left_read != right_read {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
        if left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
    }
}

fn diff_status_order(status: &str) -> u8 {
    match status {
        "modified" => 0,
        "added" => 1,
        "deleted" => 2,
        _ => 3,
    }
}

fn build_workspace_diff_entries(
    left_root: &FsPath,
    right_root: &FsPath,
) -> Result<Vec<ApiWorkspaceDiffEntry>, String> {
    let left_files = index_workspace_files(left_root)?;
    let right_files = index_workspace_files(right_root)?;
    let paths = left_files
        .keys()
        .chain(right_files.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut entries = Vec::new();

    for path in paths {
        match (left_files.get(&path), right_files.get(&path)) {
            (Some(left), None) => entries.push(ApiWorkspaceDiffEntry {
                path,
                status: "deleted".to_string(),
                is_text: left.is_text,
                left_mime: left.mime.clone(),
                right_mime: None,
                left_size: Some(left.size),
                right_size: None,
            }),
            (None, Some(right)) => entries.push(ApiWorkspaceDiffEntry {
                path,
                status: "added".to_string(),
                is_text: right.is_text,
                left_mime: None,
                right_mime: right.mime.clone(),
                left_size: None,
                right_size: Some(right.size),
            }),
            (Some(left), Some(right)) => {
                if files_identical(&left.absolute_path, &right.absolute_path)? {
                    continue;
                }

                entries.push(ApiWorkspaceDiffEntry {
                    path,
                    status: "modified".to_string(),
                    is_text: left.is_text && right.is_text,
                    left_mime: left.mime.clone(),
                    right_mime: right.mime.clone(),
                    left_size: Some(left.size),
                    right_size: Some(right.size),
                });
            }
            (None, None) => {}
        }
    }

    entries.sort_by(|left, right| {
        diff_status_order(&left.status)
            .cmp(&diff_status_order(&right.status))
            .then_with(|| left.path.cmp(&right.path))
    });

    Ok(entries)
}

fn read_workspace_file_payload(
    root_id: &str,
    root: &FsPath,
    target: &FsPath,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<ApiWorkspaceFile, String> {
    let metadata = std::fs::metadata(target).map_err(|error| {
        format!(
            "failed to stat workspace file '{}': {}",
            target.display(),
            error
        )
    })?;
    let mime = guess_mime(target);
    let is_text = detect_text_from_path(target, mime.as_deref())?;
    let modified_at = metadata.modified().ok().map(system_time_to_iso);

    if !is_text {
        return Ok(ApiWorkspaceFile {
            root_id: root_id.to_string(),
            path: relative_path(root, target),
            name: basename(target),
            size: metadata.len(),
            modified_at,
            mime,
            is_text: false,
            line_start: 0,
            line_end: 0,
            total_lines: 0,
            truncated: false,
            content: None,
        });
    }

    if metadata.len() as usize > MAX_TEXT_PREVIEW_BYTES {
        return Ok(ApiWorkspaceFile {
            root_id: root_id.to_string(),
            path: relative_path(root, target),
            name: basename(target),
            size: metadata.len(),
            modified_at,
            mime,
            is_text: true,
            line_start: 0,
            line_end: 0,
            total_lines: 0,
            truncated: true,
            content: None,
        });
    }

    let (content, line_start, line_end, total_lines, truncated) =
        read_text_preview_slice(target, line_start, line_end)?;

    Ok(ApiWorkspaceFile {
        root_id: root_id.to_string(),
        path: relative_path(root, target),
        name: basename(target),
        size: metadata.len(),
        modified_at,
        mime,
        is_text: true,
        line_start,
        line_end,
        total_lines,
        truncated,
        content: Some(content),
    })
}

fn map_diff_file_side(file: ApiWorkspaceFile) -> ApiWorkspaceDiffFileSide {
    ApiWorkspaceDiffFileSide {
        root_id: file.root_id,
        path: file.path,
        name: file.name,
        size: file.size,
        modified_at: file.modified_at,
        mime: file.mime,
        is_text: file.is_text,
        truncated: file.truncated,
        content: file.content,
    }
}

fn read_diff_side(
    root_id: &str,
    root_path: &FsPath,
    relative_path: &str,
) -> Result<Option<ApiWorkspaceDiffFileSide>, String> {
    let target = resolve_workspace_path(root_path, relative_path)?;
    if !target.exists() {
        return Ok(None);
    }
    if !target.is_file() {
        return Err(format!(
            "workspace diff path is not a file: {}",
            relative_path.trim()
        ));
    }

    let payload = read_workspace_file_payload(root_id, root_path, &target, None, None)?;
    Ok(Some(map_diff_file_side(payload)))
}

fn compute_text_change_counts(before: &str, after: &str) -> (u32, u32) {
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    (diff.count_additions(), diff.count_removals())
}

async fn list_roots_for_project(project_id: i64) -> Result<Vec<ApiWorkspaceRoot>, String> {
    let thread_store = ShepherdThreadStore::open()
        .await
        .map_err(|error| format!("failed to open thread store: {}", error))?;
    let threads = thread_store
        .list_project_threads(project_id)
        .await
        .map_err(|error| format!("failed to load threads: {}", error))?;

    // Detect the local central branch and the upstream branch, if any.
    let (current_branch, remote_branch) = match ensure_project_workspace(project_id).await {
        Ok(ws) => {
            let branch = get_current_branch(&ws.central_dir).ok();
            let remote = resolve_project_remote_branch(&ws);
            (branch, remote)
        }
        Err(_) => (None, None),
    };

    let mut roots = Vec::new();

    // Remote (upstream)
    if let Some(remote_branch) = remote_branch {
        roots.push(ApiWorkspaceRoot {
            id: "remote".to_string(),
            kind: "remote".to_string(),
            label: "Remote".to_string(),
            status: "ready".to_string(),
            summary: Some(format!("origin/{}", remote_branch)),
            thread_id: None,
            branch: Some(remote_branch),
            read_only: true,
        });
    }

    // Shepherd workspace
    roots.push(ApiWorkspaceRoot {
        id: "main".to_string(),
        kind: "main".to_string(),
        label: "Shepherd".to_string(),
        status: "ready".to_string(),
        summary: current_branch.as_deref().map(|b| b.to_string()),
        thread_id: None,
        branch: current_branch,
        read_only: false,
    });

    roots.extend(threads.into_iter().map(|thread| ApiWorkspaceRoot {
        id: format!("thread:{}", thread.id),
        kind: "thread".to_string(),
        label: thread.title,
        status: thread.status,
        summary: if thread.summary.trim().is_empty() {
            Some(thread.objective)
        } else {
            Some(thread.summary)
        },
        thread_id: Some(thread.id),
        branch: None,
        read_only: false,
    }));

    Ok(roots)
}

fn manual_search_root(
    root_id: &str,
    root_path: &FsPath,
    query: &str,
    limit: usize,
) -> Result<(Vec<ApiWorkspaceSearchResult>, bool), String> {
    let mut results = Vec::new();
    let smart_case = query.chars().any(|ch| ch.is_ascii_uppercase());
    let lowered = (!smart_case).then(|| query.to_ascii_lowercase());

    for entry in WalkDir::new(root_path)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != OsStr::new(".git"))
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        if !detect_text(&bytes, guess_mime(entry.path()).as_deref()) {
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };

        for (index, line) in content.lines().enumerate() {
            let matches = if let Some(lowered_query) = lowered.as_ref() {
                line.to_ascii_lowercase().find(lowered_query)
            } else {
                line.find(query)
            };
            if let Some(column) = matches {
                results.push(ApiWorkspaceSearchResult {
                    root_id: root_id.to_string(),
                    path: relative_path(root_path, entry.path()),
                    line: index + 1,
                    column: column + 1,
                    preview: line.trim_end().to_string(),
                });
                if results.len() >= limit {
                    return Ok((results, true));
                }
            }
        }
    }

    Ok((results, false))
}

fn ripgrep_search_root(
    root_id: &str,
    root_path: &FsPath,
    query: &str,
    limit: usize,
) -> Result<(Vec<ApiWorkspaceSearchResult>, bool), String> {
    let output = Command::new("rg")
        .current_dir(root_path)
        .args([
            "--json",
            "--line-number",
            "--column",
            "--color",
            "never",
            "--hidden",
            "--smart-case",
            "--fixed-strings",
            "--glob",
            "!.git",
            "--glob",
            "!.git/**",
            "--",
            query,
            ".",
        ])
        .output();

    let output = match output {
        Ok(output) => output,
        Err(_) => return manual_search_root(root_id, root_path, query, limit),
    };

    if !output.status.success() && output.status.code() != Some(1) {
        return manual_search_root(root_id, root_path, query, limit);
    }

    let mut results = Vec::new();
    let mut truncated = false;

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("type").and_then(|item| item.as_str()) != Some("match") {
            continue;
        }

        let Some(data) = value.get("data") else {
            continue;
        };
        let Some(path) = data
            .get("path")
            .and_then(|path| path.get("text"))
            .and_then(|text| text.as_str())
        else {
            continue;
        };
        let Some(line_number) = data.get("line_number").and_then(|item| item.as_u64()) else {
            continue;
        };
        let Some(line_text) = data
            .get("lines")
            .and_then(|lines| lines.get("text"))
            .and_then(|text| text.as_str())
        else {
            continue;
        };
        let column = data
            .get("submatches")
            .and_then(|items| items.as_array())
            .and_then(|items| items.first())
            .and_then(|item| item.get("start"))
            .and_then(|value| value.as_u64())
            .map(|value| value as usize + 1)
            .unwrap_or(1);

        results.push(ApiWorkspaceSearchResult {
            root_id: root_id.to_string(),
            path: path.replace('\\', "/"),
            line: line_number as usize,
            column,
            preview: line_text.trim_end().to_string(),
        });

        if results.len() >= limit {
            truncated = true;
            break;
        }
    }

    Ok((results, truncated))
}

pub async fn list_workspace_roots(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let roots = list_roots_for_project(project_id)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(roots))
}

pub async fn list_workspace_tree(
    Path(project_id): Path<i64>,
    Query(query): Query<WorkspaceTreeQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let root_path = resolve_root_path(project_id, &query.root_id)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let target = resolve_workspace_path(&root_path, query.path.as_deref().unwrap_or_default())
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    if !target.exists() || !target.is_dir() {
        return Err((
            StatusCode::NOT_FOUND,
            format!(
                "workspace directory not found: {}",
                query.path.as_deref().unwrap_or_default().trim()
            ),
        ));
    }

    let index = workspace_root_index(&root_path)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let key = relative_path(&root_path, &target);
    let entries = index
        .directories
        .get(&key)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|entry| to_api_tree_entry(&query.root_id, entry))
        .collect::<Vec<_>>();

    Ok(Json(ApiWorkspaceTree {
        root_id: query.root_id,
        path: relative_path(&root_path, &target),
        entries,
    }))
}

pub async fn complete_workspace_path_entries(
    Path(project_id): Path<i64>,
    Query(query): Query<WorkspaceCompleteQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let root_path = resolve_root_path(project_id, &query.root_id)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let entries = complete_workspace_path(&root_path, query.prefix.as_deref().unwrap_or_default())
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    Ok(Json(entries))
}

pub async fn get_workspace_file(
    Path(project_id): Path<i64>,
    Query(query): Query<WorkspaceFileQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let root_path = resolve_root_path(project_id, &query.root_id)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let target = resolve_workspace_path(&root_path, &query.path)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    if !target.exists() || !target.is_file() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("workspace file not found: {}", query.path.trim()),
        ));
    }

    let payload = read_workspace_file_payload(
        &query.root_id,
        &root_path,
        &target,
        query.line_start,
        query.line_end,
    )
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    Ok(Json(payload))
}

pub async fn save_workspace_file(
    Path(project_id): Path<i64>,
    Json(body): Json<SaveWorkspaceFileBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if matches!(parse_root_id(&body.root_id), Ok(WorkspaceRootRef::Remote)) {
        return Err((
            StatusCode::FORBIDDEN,
            "remote root is read-only".to_string(),
        ));
    }
    let root_path = resolve_root_path(project_id, &body.root_id)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let target = resolve_workspace_path(&root_path, &body.path)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;

    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to create '{}': {}", parent.display(), error),
            )
        })?;
    }

    tokio::fs::write(&target, body.content.into_bytes())
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "failed to write workspace file '{}': {}",
                    target.display(),
                    error
                ),
            )
        })?;
    invalidate_workspace_index(&root_path);

    Ok(Json(WorkspaceWriteResponse {
        ok: true,
        root_id: body.root_id,
        path: relative_path(&root_path, &target),
    }))
}

pub async fn upload_workspace_files(
    Path(project_id): Path<i64>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut root_id: Option<String> = None;
    let mut base_path = String::new();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid upload body: {}", error),
        )
    })? {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "root_id" => {
                let value = field.text().await.map_err(|error| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("invalid root_id field: {}", error),
                    )
                })?;
                root_id = Some(value);
            }
            "path" => {
                base_path = field.text().await.map_err(|error| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("invalid path field: {}", error),
                    )
                })?;
            }
            _ => {
                let filename = sanitize_upload_name(field.file_name().unwrap_or_default())
                    .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
                let bytes = field.bytes().await.map_err(|error| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("failed to read uploaded file '{}': {}", filename, error),
                    )
                })?;
                files.push((filename, bytes.to_vec()));
            }
        }
    }

    let root_id = root_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "root_id is required".to_string()))?;
    if files.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "upload contained no files".to_string(),
        ));
    }

    let root_path = resolve_root_path(project_id, &root_id)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let target_dir = resolve_workspace_path(&root_path, &base_path)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;

    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "failed to create upload target '{}': {}",
                    target_dir.display(),
                    error
                ),
            )
        })?;

    for (filename, bytes) in files {
        let target = target_dir.join(filename);
        tokio::fs::write(&target, bytes).await.map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "failed to write uploaded file '{}': {}",
                    target.display(),
                    error
                ),
            )
        })?;
    }
    invalidate_workspace_index(&root_path);

    Ok(Json(WorkspaceWriteResponse {
        ok: true,
        root_id,
        path: relative_path(&root_path, &target_dir),
    }))
}

pub async fn download_workspace_file(
    Path(project_id): Path<i64>,
    Query(query): Query<WorkspaceFileQuery>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let root_path = resolve_root_path(project_id, &query.root_id)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let target = resolve_workspace_path(&root_path, &query.path)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    if !target.exists() || !target.is_file() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("workspace file not found: {}", query.path.trim()),
        ));
    }

    let bytes = std::fs::read(&target).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "failed to read workspace file '{}': {}",
                target.display(),
                error
            ),
        )
    })?;
    let headers = build_download_headers(&target, guess_mime(&target).as_deref())
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    Ok((headers, bytes).into_response())
}

pub async fn get_workspace_diff(
    Path(project_id): Path<i64>,
    Query(query): Query<WorkspaceDiffQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if query.left_root_id.trim() == query.right_root_id.trim() {
        return Err((
            StatusCode::BAD_REQUEST,
            "diff requires two different roots".to_string(),
        ));
    }

    let left_root_path = resolve_root_path(project_id, &query.left_root_id)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let right_root_path = resolve_root_path(project_id, &query.right_root_id)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let entries = build_workspace_diff_entries(&left_root_path, &right_root_path)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;

    Ok(Json(ApiWorkspaceDiffSummary {
        left_root_id: query.left_root_id,
        right_root_id: query.right_root_id,
        entries,
    }))
}

pub async fn get_workspace_diff_file(
    Path(project_id): Path<i64>,
    Query(query): Query<WorkspaceDiffFileQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if query.left_root_id.trim() == query.right_root_id.trim() {
        return Err((
            StatusCode::BAD_REQUEST,
            "diff requires two different roots".to_string(),
        ));
    }

    let left_root_path = resolve_root_path(project_id, &query.left_root_id)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let right_root_path = resolve_root_path(project_id, &query.right_root_id)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;

    let left = read_diff_side(&query.left_root_id, &left_root_path, &query.path)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let right = read_diff_side(&query.right_root_id, &right_root_path, &query.path)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;

    let status = match (left.is_some(), right.is_some()) {
        (true, true) => "modified",
        (true, false) => "deleted",
        (false, true) => "added",
        (false, false) => {
            return Err((
                StatusCode::NOT_FOUND,
                format!("workspace diff file not found: {}", query.path.trim()),
            ))
        }
    };

    let is_text = left.as_ref().map(|side| side.is_text).unwrap_or(false)
        && right.as_ref().map(|side| side.is_text).unwrap_or(false)
        || left.is_none() && right.as_ref().map(|side| side.is_text).unwrap_or(false)
        || right.is_none() && left.as_ref().map(|side| side.is_text).unwrap_or(false);

    let (additions, deletions) = if is_text {
        match (
            left.as_ref().and_then(|side| side.content.as_deref()),
            right.as_ref().and_then(|side| side.content.as_deref()),
        ) {
            (Some(before), Some(after)) => {
                let (additions, deletions) = compute_text_change_counts(before, after);
                (Some(additions), Some(deletions))
            }
            (Some(before), None) if right.is_none() => {
                let (additions, deletions) = compute_text_change_counts(before, "");
                (Some(additions), Some(deletions))
            }
            (None, Some(after)) if left.is_none() => {
                let (additions, deletions) = compute_text_change_counts("", after);
                (Some(additions), Some(deletions))
            }
            _ => (None, None),
        }
    } else {
        (None, None)
    };

    Ok(Json(ApiWorkspaceDiffFile {
        left_root_id: query.left_root_id,
        right_root_id: query.right_root_id,
        path: query.path,
        status: status.to_string(),
        is_text,
        additions,
        deletions,
        left,
        right,
    }))
}

pub async fn search_workspace(
    Path(project_id): Path<i64>,
    Query(query): Query<WorkspaceSearchQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let needle = query.q.trim();
    if needle.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "search query is required".to_string(),
        ));
    }

    let root_ids = if let Some(root_id) = query.root_id.filter(|value| !value.trim().is_empty()) {
        vec![root_id]
    } else {
        list_roots_for_project(project_id)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?
            .into_iter()
            .map(|root| root.id)
            .collect()
    };

    let mut results = Vec::new();
    let mut truncated = false;

    for root_id in root_ids {
        if results.len() >= MAX_SEARCH_RESULTS {
            truncated = true;
            break;
        }

        let root_path = resolve_root_path(project_id, &root_id)
            .await
            .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
        let remaining = MAX_SEARCH_RESULTS.saturating_sub(results.len());
        let (mut root_results, root_truncated) =
            ripgrep_search_root(&root_id, &root_path, needle, remaining)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        results.append(&mut root_results);
        truncated |= root_truncated;
    }

    Ok(Json(ApiWorkspaceSearchResponse {
        query: needle.to_string(),
        results,
        truncated,
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        build_workspace_diff_entries, compute_text_change_counts, detect_text, parse_root_id,
        resolve_workspace_path, sanitize_upload_name,
    };
    use tempfile::tempdir;

    #[test]
    fn parse_root_id_accepts_main_and_threads() {
        assert!(matches!(
            parse_root_id("main"),
            Ok(super::WorkspaceRootRef::Main)
        ));
        assert!(matches!(
            parse_root_id("thread:abc"),
            Ok(super::WorkspaceRootRef::Thread(id)) if id == "abc"
        ));
    }

    #[test]
    fn resolve_workspace_path_rejects_escape() {
        let root = std::path::Path::new("/tmp/root");
        assert!(resolve_workspace_path(root, "../oops").is_err());
        assert!(resolve_workspace_path(root, "/etc/passwd").is_err());
        assert!(resolve_workspace_path(root, "src/lib.rs").is_ok());
    }

    #[test]
    fn sanitize_upload_name_normalizes_segments() {
        assert_eq!(
            sanitize_upload_name("nested/path/file.rs").unwrap(),
            "file.rs".to_string()
        );
        assert!(sanitize_upload_name("../").is_err());
    }

    #[test]
    fn detect_text_rejects_binary() {
        assert!(detect_text(b"hello", Some("text/plain")));
        assert!(!detect_text(b"\0\0\0", Some("application/octet-stream")));
    }

    #[test]
    fn workspace_diff_entries_detect_add_modify_delete() {
        let left = tempdir().unwrap();
        let right = tempdir().unwrap();

        std::fs::write(left.path().join("same.rs"), "fn same() {}\n").unwrap();
        std::fs::write(right.path().join("same.rs"), "fn same() {}\n").unwrap();
        std::fs::write(left.path().join("only-left.rs"), "left\n").unwrap();
        std::fs::write(right.path().join("only-right.rs"), "right\n").unwrap();
        std::fs::write(left.path().join("changed.rs"), "before\nline\n").unwrap();
        std::fs::write(right.path().join("changed.rs"), "after\nline\n").unwrap();

        let entries = build_workspace_diff_entries(left.path(), right.path()).unwrap();
        let statuses = entries
            .into_iter()
            .map(|entry| (entry.path, entry.status))
            .collect::<Vec<_>>();

        assert_eq!(
            statuses,
            vec![
                ("changed.rs".to_string(), "modified".to_string()),
                ("only-right.rs".to_string(), "added".to_string()),
                ("only-left.rs".to_string(), "deleted".to_string()),
            ]
        );
    }

    #[test]
    fn imara_diff_counts_changed_lines() {
        let (additions, deletions) = compute_text_change_counts("one\ntwo\n", "one\nthree\nfour\n");
        assert_eq!(additions, 2);
        assert_eq!(deletions, 1);
    }
}
