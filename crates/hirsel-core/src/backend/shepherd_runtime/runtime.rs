use std::path::{Path, PathBuf};

use lash::{PromptOverrideMode, PromptSectionName, PromptSectionOverride};

use super::history::{chunk_image_count, chunk_text};
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
use crate::backend::librarian::LIBRARIAN_SURREALQL_GUIDE;
use crate::backend::prompts;
use crate::backend::ProjectStore;

fn scope_label(scope: &ShepherdScope) -> String {
    match scope {
        ShepherdScope::General => "general".to_string(),
        ShepherdScope::Shepherd { project_id, .. } => format!("shepherd:{}", project_id),
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => format!("thread:{}:{}", project_id, thread_id),
        ShepherdScope::Librarian { project_id, .. } => format!("librarian:{}", project_id),
    }
}

async fn build_scope_guidance(
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    cwd: &Path,
) -> Result<String, String> {
    let focus_line = match focus {
        Some(f) => format!("Focus item: {} ({})", f.task_name, f.task_id),
        None => "Focus item: none".to_string(),
    };
    let workspace_root = cwd.display().to_string();

    let scope_header = match scope {
        ShepherdScope::Librarian { project_id, .. } => prompts::render_librarian_scope_guidance(
            *project_id,
            &workspace_root,
            LIBRARIAN_SURREALQL_GUIDE,
        )?,
        ShepherdScope::Thread {
            thread_id, title, ..
        } => prompts::render_thread_scope_guidance(title, thread_id, &focus_line, &workspace_root)?,
        _ => prompts::render_shepherd_scope_guidance(
            &scope_label(scope),
            &focus_line,
            &workspace_root,
        )?,
    };

    Ok(scope_header)
}

pub(super) async fn shepherd_prompt_overrides(
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    cwd: &Path,
) -> Vec<PromptSectionOverride> {
    vec![PromptSectionOverride {
        section: PromptSectionName::Guidance,
        block: None,
        mode: PromptOverrideMode::Append,
        content: build_scope_guidance(scope, focus, cwd)
            .await
            .unwrap_or_else(|error| format!("## Hirsel Prompt Error\n\n{error}")),
    }]
}

pub(super) async fn build_user_turn_text(
    scope: &ShepherdScope,
    chunks: &[ShepherdMessageChunk],
) -> Result<String, String> {
    let expanded = crate::backend::skills::build_user_turn_text(scope, chunks).await?;
    let base_text = if !expanded.trim().is_empty() {
        expanded
    } else {
        let image_count = chunk_image_count(chunks);
        if image_count > 0 {
            format!(
                "Please inspect the {} attached image{} and help based on what you observe.",
                image_count,
                if image_count == 1 { "" } else { "s" }
            )
        } else {
            let text = chunk_text(chunks).trim().to_string();
            if !text.is_empty() {
                text
            } else {
                "Continue.".to_string()
            }
        }
    };

    if matches!(scope, ShepherdScope::Librarian { .. }) {
        return Ok(base_text);
    }

    Ok(base_text)
}

pub(super) async fn resolve_scope_project_id(scope: &ShepherdScope) -> Option<i64> {
    match scope {
        ShepherdScope::General => None,
        ShepherdScope::Shepherd { project_id, .. } => Some(*project_id),
        ShepherdScope::Thread { project_id, .. } => Some(*project_id),
        ShepherdScope::Librarian { project_id, .. } => Some(*project_id),
    }
}

async fn resolve_project_workspace(project_id: i64) -> Result<PathBuf, String> {
    let store = ProjectStore::open()
        .await
        .map_err(|error| format!("failed to open project store: {}", error))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|error| format!("failed to load project {}: {}", project_id, error))?;
    if let Some(cwd) = project
        .shepherd_cwd
        .as_deref()
        .filter(|p| !p.trim().is_empty())
    {
        let path = PathBuf::from(cwd);
        if path.is_dir() {
            return Ok(path);
        }
    }
    for ws in &project.workspaces {
        if let Some(path_str) = ws.path.as_deref().filter(|p| !p.trim().is_empty()) {
            let path = PathBuf::from(path_str);
            if path.is_dir() {
                return Ok(path);
            }
        }
    }
    Ok(dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")))
}

pub(super) async fn resolve_scope_workspace(scope: &ShepherdScope) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("HIRSEL_SCOPE_WORKDIR")
        .map(PathBuf::from)
        .filter(|path| path.exists() && path.is_dir())
    {
        return Ok(path);
    }

    match scope {
        ShepherdScope::General => Ok(dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))),
        ShepherdScope::Shepherd { project_id, .. }
        | ShepherdScope::Librarian { project_id, .. } => {
            resolve_project_workspace(*project_id).await
        }
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => {
            // Try thread's own cwd first
            if let Ok(store) = crate::backend::ShepherdThreadStore::open().await {
                if let Ok(thread) = store.get_thread(thread_id).await {
                    if let Some(cwd) = thread.cwd.as_deref().filter(|p| !p.trim().is_empty()) {
                        let path = PathBuf::from(cwd);
                        if path.is_dir() {
                            return Ok(path);
                        }
                    }
                }
            }
            // Fall back to project workspace
            resolve_project_workspace(*project_id).await
        }
    }
}
