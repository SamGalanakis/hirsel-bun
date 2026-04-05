use std::path::{Path, PathBuf};

use lash::{PromptOverrideMode, PromptSectionName, PromptSectionOverride};

use super::history::{chunk_image_count, chunk_text};
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
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
        ShepherdScope::Librarian { project_id, .. } => format!(
            "## Hirsel Librarian\n\n\
            Project: {}\n\
            Workspace root: {}\n\n\
            ## Role\n\n\
            You are the Librarian — a background agent that maintains the project's knowledge graph. \
            You receive batches of knowledge events emitted by the Shepherd during conversations with the user. \
            Your job is to process each event and update the graph.\n\n\
            ## How to Process Events\n\n\
            For each event:\n\
            1. Read the summary and conversation context to understand what happened.\n\
            2. If files are referenced, read them to extract code structure (functions, modules, dependencies).\n\
            3. Update the graph using `graph_surql` with appropriate kinds:\n\
               - `module` — a file or logical code module\n\
               - `function` — a function, method, or endpoint\n\
               - `feature` — a user-facing capability\n\
               - `bug` — a known defect\n\
               - `idea` — a future direction or enhancement mentioned by the user\n\
               - `observation` — something noteworthy about the codebase\n\
               - `decision` — an architectural or design choice and its rationale\n\
               - `risk` — a potential problem or technical debt\n\
            4. Create relations in `kg_edge` for concepts like:\n\
               - `implements` — code that implements a feature\n\
               - `depends_on` — code dependency\n\
               - `tested_by` — test coverage\n\
               - `touches` — code modified by a thread or event\n\
               - `addresses` — work that fixes a bug or risk\n\
               - `blocks` — something preventing progress\n\
               - `relevant_to` — loose association\n\
               - `part_of` — containment (function part_of module)\n\
            5. Use `edit_graph_node_text` when refining a long text field on an existing node instead of rewriting the whole node.\n\n\
            ## Guidelines\n\n\
            - Be precise with node IDs. Use file paths for modules (`src/auth.rs`), function names for functions (`verify_token`), short slugs for concepts (`auth`, `rate_limiting`).\n\
            - Set `confidence` on ideas and observations: `high` (user was definitive), `medium` (discussed but not committed), `soft` (mentioned in passing).\n\
            - Set `source` to `user` or `shepherd` based on who originated the knowledge.\n\
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
            If the project has no `flake.nix` yet, create one before delegating any coding threads.\n\n\
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
            The canvas is an HTML panel for visual communication with the user. \
            Use it for synthesis, comparisons, diagrams, and status — not decorative filler.\n\n\
            Rules:\n\
            - Call `read_canvas` before editing. Only call `update_canvas` when project state materially changed.\n\
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
            Prefer `hirsel-fileref` for links, `hirsel-coderef` for file slices, `hirsel-code` for inline examples, \
            `hirsel-codediff` for comparisons, `hirsel-patchset` for grouped reviews, `hirsel-diagram` for Mermaid.\n",
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
