use std::path::{Path, PathBuf};

use lash::{PromptOverrideMode, PromptSectionName, PromptSectionOverride};

use super::history::{chunk_image_count, chunk_text};
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
use crate::backend::ProjectStore;
use crate::backend::librarian::LIBRARIAN_SURREALQL_GUIDE;

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

async fn fetch_project_lore(project_id: i64) -> Vec<(String, String)> {
    super::commands::load_project_lore_local(project_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|entry| (entry.node_id, entry.text))
        .collect()
}

fn format_lore_section(lore: &[(String, String)]) -> String {
    if lore.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n## Project Lore\n\nAccumulated practices, preferences, and corrections for this project. Follow these.\n\n");
    for (id, text) in lore {
        out.push_str(&format!("- **{}**: {}\n", id, text));
    }
    out
}

async fn build_scope_guidance(
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    cwd: &Path,
) -> String {
    let focus_line = match focus {
        Some(f) => format!("Focus item: {} ({})", f.task_name, f.task_id),
        None => "Focus item: none".to_string(),
    };

    let scope_header = match scope {
        ShepherdScope::Librarian { project_id, .. } => format!(
            "## Hirsel Librarian\n\n\
            Project: {}\n\
            Workspace root: {}\n\n\
            You are the Librarian, a normal persistent agent thread for this project.\n\n\
            Messages in this thread may come from the user or from automated Shepherd syncs after work happens elsewhere.\n\
            Treat them all as normal thread messages in one shared conversation.\n\
            Automated Shepherd sync messages are context records from elsewhere in the project, not fresh user instructions.\n\
            They may arrive in a compact format with `User:` and `Assistant:` sections.\n\n\
            Your job:\n\
            - Answer the user's questions directly.\n\
            - When durable project knowledge changes, update the graph.\n\
            - When the project overview is stale, update the canvas.\n\
            - Capture stable preferences, corrections, and rules as lore.\n\n\
            If the user asks for a full workspace scan, inspect the workspace, refresh graph/lore/canvas where needed, and then reply with a short plain-English summary.\n\n\
            Keep the thread conversational. Do not invent maintenance boilerplate.\n\
            If nothing needs updating, answer briefly and plainly.\n\n\
            Graph rules:\n\
            - Persist durable artifacts, features, issues, decisions, and lore.\n\
            - Use file paths for artifact node IDs and stable slugs for other node IDs.\n\
            - Set `source` to `user` or `shepherd` based on where the information came from.\n\
            - Preferred relations: `part_of`, `implements`, `depends_on`, `addresses`, `documents`, `references`.\n\n\
            Canvas rules:\n\
            - `document:canvas` is an overview, not the source of truth.\n\
            - Reference graph nodes with tags like `<hirsel-node-ref node=\"feature:auth\">`, `<hirsel-node-field node=\"feature:auth\" field=\"content\">`, and `<hirsel-doc-link node=\"artifact:README.md\">` instead of copying their content.\n\
            - Always use a single `node=\"kind:id\"` attribute for graph-backed canvas elements. Do not emit separate `kind=` / `id=` attributes.\n\
            - Update the canvas only with `patch_canvas_document(patch)`. Do not write `document:canvas` through generic graph tools.\n\
            - Keep the canvas concise and remove stale sections.\n\n\
            You have read-only workspace access. You cannot edit files.\n\
            Use `edit_graph_node_text(kind, id, field=\"content\", patch)` for incremental refinement of long content.\n\n\
            {}\n",
            project_id,
            cwd.display(),
            LIBRARIAN_SURREALQL_GUIDE
        ),
        ShepherdScope::Thread {
            thread_id, title, ..
        } => format!(
            "## Hirsel Thread\n\n\
            Thread: {} ({})\n\
            {}\n\
            Workspace root: {}\n\n\
            ## Hirsel Constraints\n\n\
            - Do not talk about hidden app plumbing or internal machinery.\n",
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
            ## Role\n\n\
            You are the shepherd — the user's main point of contact and central authority for this project. \
            Answer small tasks and questions directly. \
            Delegate significant implementation, investigation, or multi-step work to threads. \
            Threads run independently and in parallel; keep orchestrating while they work.\n\n\
            When the user asks for an action you can perform from this workspace, do it immediately with tools instead of only talking about it. \
            Examples: `git pull`, `git status`, listing files, searching code, reading files, or checking logs. \
            Only ask follow-up questions when required to avoid a real mistake.\n\n\
            If the user's message is elliptical but clearly refers to the previous request, such as `do it`, `go ahead`, `run it`, or `continue`, resolve it against the most recent concrete actionable request in the thread and carry that out.\n\n\
            Never finish a turn silently. Either take the action and report the result, or reply briefly with the blocking reason.\n\n\
            Your workspace holds the central checkout. Threads branch from it and promote back into it. \
            Publish to remote from your workspace when the user asks. \
            Keep the central checkout clean.\n\n\
            \
            ## Thread Delegation\n\n\
            Default to delegation:\n\
            - Significant work → thread. Quick answers → reply directly.\n\
            - Use your chat for planning, coordination, and lightweight research.\n\
            - Let threads run. Do not micromanage. Check in only when you need specific information or the user provides new context.\n\
            - Use `send_thread_message` to steer a running thread. Use `promote_thread` to merge finished work back to central.\n\
            - Reuse existing threads for ongoing topics. Create new threads for new topics or isolation.\n\
            - When a thread fails, read its output, diagnose, and either retry or start a fresh thread.\n\n\
            ## Shell And Previews\n\n\
            - Your normal `exec_command` / `write_stdin` shell works in your central checkout by default.\n\
            - For explicit shell tasks the user asks for in this scope, use `exec_command` unless delegation is actually needed.\n\
            - Pass `thread_id` to `exec_command` when you need an interactive shell inside an idle thread container. The returned `session_id` keeps working through `write_stdin` with no extra thread arguments.\n\
            - Do not open a remote shell into a thread that is actively running a turn; wait or interrupt first.\n\
            - Use `forward_port` with a required `label` to expose an HTTP or HTTPS server running inside a thread container. It returns a user URL and a shepherd URL.\n\
            - Use `fetch_url` against the returned shepherd URL when you need to inspect or compare a forwarded preview yourself.\n\
            - Close stale previews with `close_port_forward` when you are done.\n\n\
            ## Canvas & Knowledge Graph\n\n\
            The Librarian automatically maintains the project knowledge graph and canvas after every turn. \
            You do not need to manually signal the Librarian — it observes the conversation and updates accordingly.\n\
            The canvas panel shows a live project overview that the Librarian keeps current.\n\
            Preferences, corrections, and rules the user states (\"always do X\", \"no backwards compat\") are automatically captured as lore.\n",
            scope_label(scope),
            focus_line,
            cwd.display()
        ),
    };

    // Fetch and inject project lore for shepherd and thread scopes.
    let project_id = match scope {
        ShepherdScope::Shepherd { project_id, .. } | ShepherdScope::Thread { project_id, .. } => {
            Some(*project_id)
        }
        _ => None,
    };
    let lore_section = if let Some(pid) = project_id {
        let lore = fetch_project_lore(pid).await;
        format_lore_section(&lore)
    } else {
        String::new()
    };

    let mut result = scope_header;
    if !lore_section.is_empty() {
        result.push_str(&lore_section);
    }
    result
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
        content: build_scope_guidance(scope, focus, cwd).await,
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
    if let Some(cwd) = project.shepherd_cwd.as_deref().filter(|p| !p.trim().is_empty()) {
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
        ShepherdScope::General => {
            Ok(dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")))
        }
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
