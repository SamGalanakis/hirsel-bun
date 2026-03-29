use std::path::{Path, PathBuf};

use lash::{PromptOverrideMode, PromptSectionName, PromptSectionOverride};

use super::history::{chunk_image_count, chunk_text};
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
use crate::backend::{ensure_project_workspace, ShepherdThreadStore};

fn scope_label(scope: &ShepherdScope) -> String {
    match scope {
        ShepherdScope::General => "general".to_string(),
        ShepherdScope::Project { project_id, .. } => format!("project:{}", project_id),
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => format!("thread:{}:{}", project_id, thread_id),
    }
}

fn build_scope_guidance(
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    cwd: &Path,
) -> String {
    let bootstrap_flake = std::env::var("HIRSEL_BOOTSTRAP_FLAKE").as_deref() == Ok("1");
    let focus_line = match focus {
        Some(f) => format!("Focus item: {} ({})", f.task_name, f.task_id),
        None => "Focus item: none".to_string(),
    };

    let scope_header = match scope {
        ShepherdScope::Thread {
            thread_id, title, ..
        } => format!(
            "## Hirsel Thread\n\n\
            Thread: {} ({})\n\
            {}\n\
            Workspace root: {}\n\n\
            ## Hirsel Constraints\n\n\
            - This is a durable execution thread that the user and shepherd can inspect.\n\
            - Own the objective implied by the thread title unless shepherd redirects you.\n\
            - Use `update_plan` for substantial work so the thread card stays legible.\n\
            - Use `set_thread_status` when you become blocked, waiting, or done.\n\
            - Use `rename_thread` only when the thread purpose materially changed.\n\
            - Prefer concrete progress, decisions, and next actions over narration.\n\
            - Do not talk about hidden routing or internal machinery.\n",
            title,
            thread_id,
            focus_line,
            cwd.display()
        ),
        _ => format!(
            "## Hirsel Scope\n\n\
            Scope: {}\n\
            {}\n\
            Workspace root: {}\n\n\
            ## Hirsel Constraints\n\n\
            - The final assistant response in this scope is shown directly to the user.\n\
            - Talk plainly, decide when to answer directly, and delegate aggressively with threads when real parallel work is needed.\n\
            - Use `create_thread`, `send_thread_message`, and `read_thread_updates` to orchestrate separate lines of work.\n\
            - For simple conversational questions, answer directly in plain language without REPL code.\n\
            - If the project central checkout has no `flake.nix` yet, create one before starting normal coding work. Until that exists, do not delegate coding threads.\n\
            - The canvas is a maintained artifact. Use `read_canvas` before editing it.\n\
            - Only call `update_canvas` when project meaning materially changed.\n\
            - Canvas updates must replace the full HTML document and preserve stable structure when possible.\n\
            - The canvas is for illustrating the current situation to the user, not reiterating obvious shell context.\n\
            - Do not waste canvas space repeating the project title or generic chrome the user can already see.\n\
            - Prefer synthesis, comparisons, diagrams, and “what matters now” framing over dashboard filler.\n\
            - Inline Mermaid setup is allowed in the canvas. Do not add arbitrary third-party assets beyond Mermaid.\n\
            - Never claim work happened unless you actually executed tools.\n\
            - Never return raw tool payloads (JSON/Python dict/list) as final user-facing output.\n\
            - Summarize tool outcomes in plain language.\n\
            - For create/setup/scaffold/build/implement requests, perform real workspace mutations before finishing.",
            scope_label(scope),
            focus_line,
            cwd.display()
        ),
    };

    if bootstrap_flake {
        format!(
            "{}\n\n## Bootstrap Mode\n\n- This project central checkout has no project flake yet.\n- Create a valid `flake.nix` in the workspace before doing normal coding work.\n- Do not create or start coding threads until the project flake exists.\n",
            scope_header
        )
    } else {
        scope_header
    }
}

pub(super) fn shepherd_prompt_overrides(
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    cwd: &Path,
) -> Vec<PromptSectionOverride> {
    vec![PromptSectionOverride {
        section: PromptSectionName::Guidance,
        mode: PromptOverrideMode::Append,
        content: build_scope_guidance(scope, focus, cwd),
    }]
}

pub(super) fn build_user_turn_text(chunks: &[ShepherdMessageChunk]) -> String {
    let text = chunk_text(chunks).trim().to_string();
    if !text.is_empty() {
        return text;
    }

    let image_count = chunk_image_count(chunks);
    if image_count > 0 {
        return format!(
            "Please inspect the {} attached image{} and help based on what you observe.",
            image_count,
            if image_count == 1 { "" } else { "s" }
        );
    }

    "Continue.".to_string()
}

pub(super) async fn resolve_scope_project_id(scope: &ShepherdScope) -> Option<i64> {
    match scope {
        ShepherdScope::General => None,
        ShepherdScope::Project { project_id, .. } => Some(*project_id),
        ShepherdScope::Thread { project_id, .. } => Some(*project_id),
    }
}

async fn resolve_project_workspace(project_id: i64) -> Result<PathBuf, String> {
    let handle = ensure_project_workspace(project_id).await?;
    let path = handle.central_dir;
    if !path.exists() || !path.is_dir() {
        return Err(format!(
            "Project workspace '{}' points at missing checkout '{}'",
            handle.workspace_name,
            path.display()
        ));
    }
    Ok(path)
}

pub(super) async fn resolve_scope_workspace(scope: &ShepherdScope) -> Result<PathBuf, String> {
    match scope {
        ShepherdScope::General => Err("general scope has no workspace".to_string()),
        ShepherdScope::Project {
            project_id,
            workspace_path,
            ..
        } => {
            if let Some(path) = workspace_path.as_ref().filter(|p| !p.trim().is_empty()) {
                let path = PathBuf::from(path);
                if path.exists() && path.is_dir() {
                    return Ok(path);
                }
                return Err(format!(
                    "project scope workspace path '{}' does not exist",
                    path.display()
                ));
            }
            resolve_project_workspace(*project_id).await
        }
        ShepherdScope::Thread {
            project_id,
            thread_id,
            workspace_path,
            ..
        } => {
            if let Some(path) = workspace_path.as_ref().filter(|p| !p.trim().is_empty()) {
                let path = PathBuf::from(path);
                if path.exists() && path.is_dir() {
                    return Ok(path);
                }
                return Err(format!(
                    "thread scope workspace path '{}' does not exist",
                    path.display()
                ));
            }
            if let Ok(store) = ShepherdThreadStore::open().await {
                if let Ok(thread) = store.get_thread(thread_id).await {
                    if let Some(path) = thread
                        .workspace_path
                        .as_deref()
                        .filter(|p| !p.trim().is_empty())
                    {
                        let path = PathBuf::from(path);
                        if path.exists() && path.is_dir() {
                            return Ok(path);
                        }
                    }
                }
            }
            resolve_project_workspace(*project_id).await
        }
    }
}
