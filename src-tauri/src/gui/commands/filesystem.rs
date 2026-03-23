//! Filesystem-related GUI commands
//!
//! Commands for file/folder picking, path autocompletion, and repository validation.

use std::path::PathBuf;

use super::types::RepoValidation;
use crate::core::git;

/// Open a native folder picker dialog
///
/// Returns the selected folder path, or None if cancelled.
#[tracing::instrument]
#[tauri::command]
pub async fn pick_folder() -> Result<Option<String>, String> {
    let result = rfd::AsyncFileDialog::new()
        .set_title("Select Folder")
        .pick_folder()
        .await;

    Ok(result.map(|f| f.path().to_string_lossy().to_string()))
}

/// Expand ~ to home directory
fn expand_home(path: &str) -> PathBuf {
    if path.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            if path == "~" {
                return home;
            } else if let Some(rest) = path.strip_prefix("~/") {
                return home.join(rest);
            }
        }
    }
    PathBuf::from(path)
}

/// Get path suggestions for autocomplete
///
/// Given a partial path, returns a list of matching directories.
#[tracing::instrument]
#[tauri::command]
pub async fn suggest_paths(partial: String) -> Result<Vec<String>, String> {
    // Handle empty input
    if partial.is_empty() {
        return Ok(vec![]);
    }

    // Handle ~ expansion
    if partial == "~" {
        if let Some(home) = dirs::home_dir() {
            return Ok(vec![home.to_string_lossy().to_string() + "/"]);
        }
        return Ok(vec![]);
    }

    // Expand ~ in the path
    let expanded = expand_home(&partial);
    let expanded_str = expanded.to_string_lossy().to_string();

    // Determine the directory to list and the prefix to filter by
    let (dir_to_list, prefix): (PathBuf, String) = if expanded_str.ends_with('/') {
        // User typed a complete directory path, list its contents
        (expanded, String::new())
    } else {
        // User is typing a name, list parent directory and filter
        let parent = expanded
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/"));
        let filename = expanded
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        (parent, filename)
    };

    // List directory contents
    let mut suggestions = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&dir_to_list) {
        for entry in entries.filter_map(|e| e.ok()) {
            let entry_path = entry.path();

            // Only suggest directories
            if !entry_path.is_dir() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden directories unless user is typing a dot
            if name.starts_with('.') && !prefix.starts_with('.') {
                continue;
            }

            // Filter by prefix (case-insensitive)
            if prefix.is_empty() || name.to_lowercase().starts_with(&prefix.to_lowercase()) {
                // Return full path with trailing slash for directories
                let full_path = entry_path.to_string_lossy().to_string() + "/";
                suggestions.push(full_path);
            }
        }
    }

    // Sort suggestions alphabetically
    suggestions.sort_by_key(|a| a.to_lowercase());

    // Limit to 10 suggestions
    suggestions.truncate(10);

    Ok(suggestions)
}

/// Validate a repository path or URL.
#[tracing::instrument]
#[tauri::command]
pub async fn validate_repo(path: String) -> Result<RepoValidation, String> {
    let path = path.trim();

    if path.is_empty() {
        return Ok(RepoValidation {
            valid: false,
            error: Some("Path is required".to_string()),
            is_remote: false,
            branches: Vec::new(),
            current_branch: None,
            repo_url: String::new(),
            url_branch: None,
            url_branch_valid: false,
            needs_dir_create: false,
            needs_git_init: false,
        });
    }

    if git::is_remote_url(path) {
        let parsed = git::parse_github_url(path);
        let repo_url = parsed.repo_url.clone();
        return match git::list_remote_branches(&repo_url) {
            Ok(branches) => Ok(RepoValidation {
                valid: true,
                error: None,
                is_remote: true,
                url_branch_valid: parsed
                    .branch
                    .as_ref()
                    .map(|branch| branches.contains(branch))
                    .unwrap_or(false),
                branches,
                current_branch: None,
                repo_url,
                url_branch: parsed.branch,
                needs_dir_create: false,
                needs_git_init: false,
            }),
            Err(error) => Ok(RepoValidation {
                valid: false,
                error: Some(format!("Failed to access repository: {}", error)),
                is_remote: true,
                branches: Vec::new(),
                current_branch: None,
                repo_url,
                url_branch: parsed.branch,
                url_branch_valid: false,
                needs_dir_create: false,
                needs_git_init: false,
            }),
        };
    }

    let local_path = std::path::Path::new(path);
    if !local_path.exists() {
        return Ok(RepoValidation {
            valid: false,
            error: None,
            is_remote: false,
            branches: Vec::new(),
            current_branch: None,
            repo_url: path.to_string(),
            url_branch: None,
            url_branch_valid: false,
            needs_dir_create: true,
            needs_git_init: true,
        });
    }

    let git_dir = local_path.join(".git");
    if !git_dir.exists() {
        return Ok(RepoValidation {
            valid: false,
            error: None,
            is_remote: false,
            branches: Vec::new(),
            current_branch: None,
            repo_url: path.to_string(),
            url_branch: None,
            url_branch_valid: false,
            needs_dir_create: false,
            needs_git_init: true,
        });
    }

    match git::list_branches(local_path) {
        Ok(branches) => Ok(RepoValidation {
            valid: true,
            error: None,
            is_remote: false,
            branches,
            current_branch: git::get_current_branch(local_path).ok(),
            repo_url: path.to_string(),
            url_branch: None,
            url_branch_valid: false,
            needs_dir_create: false,
            needs_git_init: false,
        }),
        Err(error) => Ok(RepoValidation {
            valid: false,
            error: Some(format!("Failed to read repository: {}", error)),
            is_remote: false,
            branches: Vec::new(),
            current_branch: None,
            repo_url: path.to_string(),
            url_branch: None,
            url_branch_valid: false,
            needs_dir_create: false,
            needs_git_init: false,
        }),
    }
}
