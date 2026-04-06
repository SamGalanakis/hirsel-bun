use std::path::{Path, PathBuf};

use lash::{PromptOverrideMode, PromptSectionName, PromptSectionOverride};

use super::history::{chunk_image_count, chunk_text};
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
use crate::backend::db::global_db;
use crate::backend::librarian::LIBRARIAN_SURREALQL_GUIDE;
use crate::backend::{ensure_project_workspace, ShepherdThreadStore};

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
    use surrealdb::types::SurrealValue;

    let db = global_db().await;
    #[derive(serde::Deserialize, SurrealValue)]
    struct LoreRow {
        node_id: String,
        summary: Option<String>,
        label: Option<String>,
    }

    let result: Result<Vec<LoreRow>, _> = db
        .query("SELECT node_id, label, summary FROM kg_node WHERE project_id = $project_id AND kind = 'lore' ORDER BY updated_at DESC LIMIT 40")
        .bind(("project_id", project_id))
        .await
        .and_then(|mut r| r.take(0));
    let rows = match result {
        Err(_) => return Vec::new(),
        Ok(rows) => rows,
    };
    rows.into_iter()
        .filter_map(|row| {
            let text = row
                .summary
                .filter(|s| !s.trim().is_empty())
                .or_else(|| row.label.filter(|s| !s.trim().is_empty()))?;
            Some((row.node_id, text))
        })
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
            You are the Librarian — a background agent that maintains the project's knowledge graph and the single canvas document. \
            You receive batches of knowledge events emitted by the Shepherd during conversations with the user. \
            Your job is to process the batch, update the graph, and update the canvas document when the batch explicitly calls for an illustration or canvas refresh.\n\n\
            ## How to Process Events\n\n\
            For each batch:\n\
            1. Read the event summaries and the provided shared conversation context once. Do not ask the Shepherd to duplicate that context inside an event.\n\
            2. If files are referenced, read them to extract durable project knowledge.\n\
            3. Update the graph using a small ontology:\n\
               - `artifact` — code, config, test, doc, script, or other project artifact\n\
               - `feature` — a user-facing capability or important subsystem responsibility\n\
               - `issue` — a bug, risk, defect, or technical problem\n\
               - `decision` — an architectural or design choice and its rationale\n\
               - `lore` — a best practice, preference, correction, anti-pattern, or operational rule learned from experience. Use stable slugs as IDs (e.g. `no_backwards_compat`, `prefer_small_prs`). The `summary` field should be a concise, actionable directive.\n\
               - `document` — a freeform HTML document, including the singular canvas document\n\
            4. Prefer these relations:\n\
               - `part_of`\n\
               - `implements`\n\
               - `depends_on`\n\
               - `addresses`\n\
               - `documents`\n\
               - `references`\n\
            5. Keep facts canonical on non-document nodes. Documents explain or illustrate other nodes; they do not replace them.\n\
            6. The canvas is the single `document:canvas` node. When a batch asks for an illustration or canvas refresh, update that node's `body_html`.\n\
            7. Inline node references inside document HTML are canonical. Use components like `<hirsel-node-ref node=\"feature:auth\">`, `<hirsel-node-field node=\"decision:event_queue\" field=\"summary\">`, `<hirsel-node-list node=\"feature:auth\" relation=\"implements\">`, and `<hirsel-doc-target node=\"feature:auth\">`. The backend derives `references` and `documents` edges from these automatically. Do not duplicate that work manually.\n\
            8. Use `edit_graph_node_text` when refining a long existing text field instead of rewriting the whole node.\n\n\
            ## Guidelines\n\n\
            - Be precise with node IDs. Use file paths for artifact files (`src/auth.rs`), stable slugs for features/issues/decisions (`auth`, `event_queue`, `retry_backoff`), and `canvas` for the singular canvas document.\n\
            - Set `source` to `user` or `shepherd` based on who originated the knowledge.\n\
            - Update the canvas only when the event batch explicitly calls for it. Keeping the canvas current is not the Shepherd's tool job; it is your document-writing job when asked.\n\
            - Be concise. Process the batch and stop. Do not narrate your actions.\n\
            - You have read-only workspace access. You cannot edit files.\n\n\
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
            - Do not talk about hidden app plumbing or internal machinery.\n\
            - If you discover a reusable practice, gotcha, or correction during your work, emit a knowledge event with `kind: \"lore\"` so the Librarian captures it for future threads.\n",
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
            ## Canvas\n\n\
            The canvas is a single project document rendered in the canvas panel. \
            Use it for synthesis, comparisons, diagrams, and status — not decorative filler.\n\n\
            Rules:\n\
            - You do not edit the canvas directly. If the user wants a new illustration, diagram, or visual summary, emit a knowledge event telling the Librarian what the canvas should show.\n\
            - The Librarian already has the surrounding chat context. Do not duplicate chat context inside the event summary.\n\
            - Write an HTML fragment. No `<html>`, `<head>`, or `<body>` tags.\n\
            - CSS is auto-scoped. Use theme vars: `hsl(var(--background))`, `--foreground`, `--card`, `--border`, `--ring`, `--signal-blue`, `--signal-amber`, `--signal-green`, `--signal-red`.\n\
            - Scripts: inline JS only, no imports or network. `canvasRoot` points to the canvas root. `mermaid` is preloaded.\n\n\
            ### Components\n\n\
            All components accepting `tone` support: muted, info, success, warning, danger.\n\n\
            | Element | Key attrs | Notes |\n\
            |---|---|---|\n\
            | `hirsel-card` | heading, eyebrow, tone | General-purpose container |\n\
            | `hirsel-callout` | title, tone | Highlighted message block |\n\
            | `hirsel-fileref` | path, root-id, line-start, line-end, status | Lightweight workspace link |\n\
            | `hirsel-filelist` | title, root-id | Contains `<li path=\"...\" status=\"...\">` items |\n\
            | `hirsel-code` | title, filename, language, line-start | Inline code block |\n\
            | `hirsel-codediff` | title, language | Contains `<pre data-side=\"before\" label=\"...\">` and `after` |\n\
            | `hirsel-coderef` | path, line-start, line-end, root-id | Renders a file slice from the workspace |\n\
            | `hirsel-patchset` | title, summary | Contains `<section path=\"...\" status=\"...\" summary=\"...\">` |\n\
            | `hirsel-diagram` | title | Mermaid diagram. Alias: `hirsel-dia` |\n\
            | `hirsel-stat-grid` | — | Contains `<hirsel-stat label=\"...\" value=\"...\" detail=\"...\">` |\n\
            | `hirsel-tabs` | — | Contains `<section label=\"...\">` panels |\n\
            | `hirsel-disclosure` | title, tone, open | Collapsible section |\n\
            | `hirsel-progress` | label, value, max, detail, tone | Progress bar |\n\n\
            | `hirsel-node-ref` | node | Render a linked graph node chip |\n\
            | `hirsel-node-field` | node, field | Render one field from a graph node |\n\
            | `hirsel-node-list` | node, relation | Render related graph nodes |\n\
            | `hirsel-doc-target` | node | Mark what the document is explicitly about |\n\
            | `hirsel-doc-link` | node | Clickable card linking to another document node |\n\
            | `hirsel-doc-embed` | node | Embed another document's full body inline |\n\n\
            Prefer `hirsel-fileref` for links, `hirsel-coderef` for file slices, `hirsel-code` for inline examples, \
            `hirsel-codediff` for comparisons, `hirsel-patchset` for grouped reviews, `hirsel-diagram` for Mermaid.\n\n\
            ## Capturing Lore\n\n\
            When the user states a preference, correction, best practice, or rule for how work should be done in this project, \
            emit a knowledge event so the Librarian stores it as a `lore` node. Examples:\n\
            - \"always do X\" / \"never do Y\" / \"prefer X over Y\"\n\
            - \"no need for backwards compatibility\" / \"don't add shims\"\n\
            - \"we use snake_case for API fields\" / \"tests must hit the real DB\"\n\
            - Corrections: \"that approach broke last time because...\" / \"don't mock the database here\"\n\n\
            Emit the event concisely: `kind: \"lore\"`, `summary:` the rule in one sentence. The Librarian will persist it. \
            You do not need to confirm with the user — just capture and move on.\n",
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
