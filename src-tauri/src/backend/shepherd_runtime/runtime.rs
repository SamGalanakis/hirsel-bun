use std::path::{Path, PathBuf};

use lash::{PromptOverrideMode, PromptSectionName, PromptSectionOverride};

use super::history::{chunk_image_count, chunk_text};
use super::rpc::load_project_lore_via_server_control;
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
#[cfg(feature = "host")]
use crate::backend::ensure_project_workspace;
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
    load_project_lore_via_server_control(project_id)
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
            ## Role\n\n\
            You are the Librarian — a background agent that maintains the project's knowledge graph and canvas. \
            You are triggered automatically after every shepherd and thread turn. \
            You receive the recent conversation context and decide what to update.\n\n\
            ## Two-Phase Process\n\n\
            Every time you are triggered:\n\n\
            **Phase 1 — Update the graph.** Scan the conversation for anything worth persisting:\n\
            - New or changed artifacts, features, decisions, issues\n\
            - Lore: preferences, corrections, rules the user stated (\"always do X\", \"never do Y\")\n\
            - Relationships between nodes\n\
            - Prune nodes that are no longer accurate\n\n\
            **Phase 2 — Refresh the canvas.** The canvas (`document:canvas`) is a whiteboard — it does not hold knowledge itself, \
            it arranges and illustrates knowledge that lives in the graph. Think of it like a museum curator's wall: \
            the artifacts are in the vault (graph nodes), the wall (canvas) presents them with context, layout, and narrative.\n\n\
            When refreshing the canvas:\n\
            - Reference existing graph nodes with `<hirsel-node-ref>`, `<hirsel-node-field>`, `<hirsel-doc-link>` — do not duplicate their content.\n\
            - Show what matters now: current architecture, active decisions, recent changes, key relationships.\n\
            - Use diagrams (`<hirsel-diagram>`) to illustrate non-obvious connections.\n\
            - Keep it concise and scannable. Remove stale sections.\n\
            - If nothing meaningful changed, skip the canvas update.\n\n\
            ## Node Ontology\n\n\
            Every node has: `kind`, `node_id`, `label`, `content`, `source`, `metadata`, `updated_at`.\n\
            The `content` field is the single text body — plain text for most kinds, HTML for documents.\n\n\
            - `artifact` — code, config, test, doc, script, or other project artifact\n\
            - `feature` — a user-facing capability or important subsystem responsibility\n\
            - `issue` — a bug, risk, defect, or technical problem\n\
            - `decision` — an architectural or design choice and its rationale\n\
            - `lore` — a best practice, preference, correction, or operational rule. Use stable slug IDs. Content should be a concise, actionable directive.\n\
            - `document` — an HTML document. The `content` field holds the HTML body.\n\n\
            Preferred relations: `part_of`, `implements`, `depends_on`, `addresses`, `documents`, `references`.\n\n\
            ## Canvas HTML Components\n\n\
            Use these inside document `content` HTML. The backend derives edges from reference tags automatically.\n\n\
            | Element | Key attrs | Notes |\n\
            |---|---|---|\n\
            | `hirsel-node-ref` | node | Linked graph node chip (renders as doc card for document nodes) |\n\
            | `hirsel-node-field` | node, field | Inline one field from a node (renders HTML for document content) |\n\
            | `hirsel-node-list` | node, relation | List of related nodes |\n\
            | `hirsel-doc-target` | node | Mark what this document is about |\n\
            | `hirsel-doc-link` | node | Clickable card linking to another document |\n\
            | `hirsel-doc-embed` | node | Embed another document's body inline |\n\
            | `hirsel-card` | heading, eyebrow, tone | Container |\n\
            | `hirsel-callout` | title, tone | Highlighted message |\n\
            | `hirsel-diagram` | title | Mermaid diagram |\n\
            | `hirsel-stat-grid` | — | Stats with `<hirsel-stat label value detail>` |\n\
            | `hirsel-disclosure` | title, tone, open | Collapsible section |\n\n\
            Tones: muted, info, success, warning, danger.\n\n\
            ## Guidelines\n\n\
            - Be precise with node IDs: file paths for artifacts (`src/auth.rs`), stable slugs for others (`auth`, `retry_backoff`).\n\
            - Set `source` to `user` or `shepherd` based on who originated the knowledge.\n\
            - Be concise. Update the graph, refresh the canvas if needed, and stop. Do not narrate your actions.\n\
            - You have read-only workspace access. You cannot edit files.\n\
            - Use `edit_graph_node_text(kind, id, field=\"content\", patch)` for incremental refinement of long content.\n\n\
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
    if !expanded.trim().is_empty() {
        return Ok(expanded);
    }

    let image_count = chunk_image_count(chunks);
    if image_count > 0 {
        return Ok(format!(
            "Please inspect the {} attached image{} and help based on what you observe.",
            image_count,
            if image_count == 1 { "" } else { "s" }
        ));
    }

    let text = chunk_text(chunks).trim().to_string();
    if !text.is_empty() {
        return Ok(text);
    }

    Ok("Continue.".to_string())
}

pub(super) async fn resolve_scope_project_id(scope: &ShepherdScope) -> Option<i64> {
    match scope {
        ShepherdScope::General => None,
        ShepherdScope::Shepherd { project_id, .. } => Some(*project_id),
        ShepherdScope::Thread { project_id, .. } => Some(*project_id),
        ShepherdScope::Librarian { project_id, .. } => Some(*project_id),
    }
}

#[cfg(feature = "host")]
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

#[cfg(not(feature = "host"))]
async fn resolve_project_workspace(project_id: i64) -> Result<PathBuf, String> {
    Err(format!(
        "project workspace {} is unavailable in worker-only builds without an explicit scope workdir",
        project_id
    ))
}

pub(super) async fn resolve_scope_workspace(scope: &ShepherdScope) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("HIRSEL_SCOPE_WORKDIR")
        .map(PathBuf::from)
        .filter(|path| path.exists() && path.is_dir())
    {
        return Ok(path);
    }

    match scope {
        ShepherdScope::General => Err("general scope has no workspace".to_string()),
        ShepherdScope::Shepherd {
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
                    "shepherd workspace path '{}' does not exist",
                    path.display()
                ));
            }
            resolve_project_workspace(*project_id).await
        }
        ShepherdScope::Thread {
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
                    "thread scope workspace path '{}' does not exist",
                    path.display()
                ));
            }
            resolve_project_workspace(*project_id).await
        }
        ShepherdScope::Librarian {
            project_id,
            workspace_path,
            ..
        } => {
            if let Some(path) = workspace_path.as_ref().filter(|p| !p.trim().is_empty()) {
                let path = PathBuf::from(path);
                if path.exists() && path.is_dir() {
                    return Ok(path);
                }
            }
            resolve_project_workspace(*project_id).await
        }
    }
}
