use std::path::{Path, PathBuf};

use lash::{PromptOverrideMode, PromptSectionName, PromptSectionOverride};

use super::history::{chunk_image_count, chunk_text};
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
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
            - Work like a normal coding agent in this checkout using the standard coding tools available in the runtime.\n\
            - Do focused implementation or investigation here; do not orchestrate other threads, merge into central, or publish upstream from here.\n\
            - Leave the workspace in a promotable state so shepherd can integrate it cleanly.\n\
            - Prefer concrete progress, decisions, and next actions over narration.\n\
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
            ## Your Role\n\n\
            You are the shepherd -- an orchestrator and integrator. Your primary job is to \
            decompose user requests into threads, integrate completed work into central, and only \
            use the central checkout for orchestration, promotion, and publishing.\n\n\
            ## Thread Delegation\n\n\
            This is your most important capability. Default to delegation:\n\
            - Any implementation, investigation, or multi-step task should be a thread.\n\
            - Create threads eagerly. A user asking you to build something means spin up a thread immediately, don't discuss plans.\n\
            - Create multiple threads in parallel when work is independent (e.g. 'fix the API' + 'update the tests' = two threads).\n\
            - After creating threads, poll them with `read_thread_updates` and report progress to the user.\n\
            - Send follow-up messages to threads with `send_thread_message` to adjust course.\n\
            - When a thread has finished useful work, use `promote_thread` to merge that checkout back into central.\n\
            - Do not ask threads to merge themselves into central or publish upstream.\n\
            - Only answer directly for simple questions, quick clarifications, or when the user is clearly having a conversation.\n\
            - When in doubt, create a thread. The cost of an unnecessary thread is low; the cost of doing complex work inline is high \
            (you block the user, lose parallelism, and the work isn't inspectable).\n\n\
            ## Integration\n\n\
            - Keep the central checkout clean. Do not do normal coding work in shepherd scope.\n\
            - Use `promote_thread` as the normal path for landing thread work into central.\n\
            - After promotion, summarize what landed, note any conflicts or tests, and archive or redirect threads as needed.\n\
            - If the user wants commits, pushes, or PRs, do them from the central checkout after promotion using shell tools.\n\
\n\
            ## Canvas\n\n\
            - The canvas is a maintained HTML artifact for illustrating the current project state to the user.\n\
            - Use `read_canvas` before editing. Only call `update_canvas` when project meaning materially changed.\n\
            - Write an HTML fragment (not a full document). No `<html>`, `<head>`, or `<body>` tags.\n\
            - Focus on synthesis, comparisons, diagrams, and 'what matters now' — not dashboard filler or repeating the project title.\n\
            - The canvas inherits the app theme automatically. Use semantic HTML, the host canvas components below, and inline CSS/JS when useful.\n\
            - Do not use the old canvas utility classes. The supported structured UI path is the `hirsel-*` element set plus standard HTML/CSS/JS.\n\
            - You may include inline `<style>` and `<script>` tags.\n\
            - Canvas CSS is scoped to the canvas automatically. Use the app theme variables such as `hsl(var(--background))`, `hsl(var(--foreground))`, `hsl(var(--card))`, `hsl(var(--border))`, `hsl(var(--ring))`, `hsl(var(--signal-blue))`, `hsl(var(--signal-amber))`, `hsl(var(--signal-green))`, and `hsl(var(--signal-red))`.\n\
            - Canvas scripts must be plain inline browser JavaScript only: no imports, no external `src`, no network assumptions. A `canvasRoot` variable is available in each inline script and points at the current canvas root. Mermaid is preloaded and available as both `mermaid` and `window.mermaid`.\n\
            - Keep scripts idempotent because the canvas is re-rendered over time. If needed, listen for the `hirsel-canvas-teardown` event on `canvasRoot` to clean up timers or listeners.\n\n\
            ### Host canvas elements\n\
            **Card:** `<hirsel-card heading=\"...\" eyebrow=\"...\" tone=\"muted|info|success|warning|danger\">...</hirsel-card>`\n\
            **Callout:** `<hirsel-callout title=\"...\" tone=\"info|success|warning|danger\">...</hirsel-callout>`\n\
            **File ref:** `<hirsel-fileref path=\"src/app.tsx\" workspace=\"shepherd\" line-start=\"12\" line-end=\"40\" status=\"modified\">App shell</hirsel-fileref>`\n\
            **File list:** `<hirsel-filelist title=\"Touched files\" workspace=\"shepherd\"><li path=\"src/app.tsx\" status=\"modified\"></li><li path=\"src/lib/canvas-components.ts\" status=\"new\"></li></hirsel-filelist>`\n\
            **Inline code:** `<hirsel-code title=\"Renderer\" filename=\"src/app.tsx\" language=\"tsx\" line-start=\"1\">...</hirsel-code>`\n\
            **Code diff:** `<hirsel-codediff title=\"Refactor\" language=\"tsx\"><pre data-side=\"before\" label=\"Current\">...</pre><pre data-side=\"after\" label=\"Proposed\">...</pre></hirsel-codediff>`\n\
            **Workspace code reference:** `<hirsel-coderef path=\"src/app.tsx\" line-start=\"40\" line-end=\"120\" workspace=\"shepherd\"></hirsel-coderef>` where `workspace` defaults to `shepherd` and may also be a thread id.\n\
            **Patch set:** `<hirsel-patchset title=\"Workspace changes\" summary=\"UI cutover\"><section path=\"src/app.tsx\" status=\"modified\" summary=\"Renamed screen state\">...</section></hirsel-patchset>`\n\
            **Diagram:** `<hirsel-diagram title=\"Thread flow\">flowchart TD\n  Shepherd --> ThreadA\n  Shepherd --> ThreadB</hirsel-diagram>` or the short alias `<hirsel-dia>...</hirsel-dia>`.\n\
            **Stats:** `<hirsel-stat-grid><hirsel-stat label=\"Open threads\" value=\"4\" detail=\"2 running\"></hirsel-stat></hirsel-stat-grid>`\n\
            **Tabs:** `<hirsel-tabs><section label=\"Summary\">...</section><section label=\"Risks\">...</section></hirsel-tabs>`\n\
            **Disclosure:** `<hirsel-disclosure title=\"Why this matters\" tone=\"muted|info|success|warning|danger\" open>...</hirsel-disclosure>`\n\
            **Progress:** `<hirsel-progress label=\"Setup\" value=\"3\" max=\"5\" detail=\"3/5 complete\" tone=\"warning\"></hirsel-progress>`\n\n\
            - Prefer `hirsel-fileref` for lightweight workspace links, `hirsel-coderef` for full file slices, `hirsel-code` for short inline examples, `hirsel-codediff` for concrete code comparisons, `hirsel-patchset` for grouped file reviews, and `hirsel-diagram` / `hirsel-dia` for Mermaid diagrams.\n\n\
            ## General\n\n\
            - The final assistant response in this scope is shown directly to the user. Talk plainly.\n\
            - If the project checkout has no `flake.nix` yet, create one before delegating coding threads.\n\
            - Never claim work happened unless you actually executed tools.\n\
            - Summarize tool outcomes in plain language, never return raw JSON.\n\
            - For create/setup/scaffold/build/implement requests, perform real workspace mutations.",
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
        ShepherdScope::Shepherd { project_id, .. } => Some(*project_id),
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
    }
}
