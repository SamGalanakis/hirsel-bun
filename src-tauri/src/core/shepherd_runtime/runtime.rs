use std::path::{Path, PathBuf};

use lash::{PromptOverrideMode, PromptSectionName, PromptSectionOverride};

use super::history::{chunk_image_count, chunk_text};
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
use crate::core::ensure_route_runtime;

pub(super) struct ShepherdLashSink;

impl ShepherdLashSink {
    pub(super) fn tool_title_kind(name: &str) -> (String, Option<String>) {
        match name {
            "list_routes" => ("Routes".to_string(), Some("search".to_string())),
            "fork_route" => ("Fork Route".to_string(), Some("execute".to_string())),
            "select_route" => ("Select Route".to_string(), Some("execute".to_string())),
            "archive_route" => ("Archive Route".to_string(), Some("execute".to_string())),
            "sync_project" => ("Sync Project".to_string(), Some("execute".to_string())),
            "get_route_work_tree" => ("Route Work Tree".to_string(), Some("search".to_string())),
            "create_work_item" | "split_work_item" | "assign_work_item" | "reopen_work_item"
            | "archive_work_item" => ("Work Item Edit".to_string(), Some("edit".to_string())),
            "read_work_item" => ("Work Item Read".to_string(), Some("read".to_string())),
            "apply_patch_work_item" => ("Work Item Edit".to_string(), Some("edit".to_string())),
            "read_project_focus_view" => ("Project Focus".to_string(), Some("read".to_string())),
            "update_project_focus_view" => {
                ("Project Focus Update".to_string(), Some("edit".to_string()))
            }
            "read_project_retained_context" => {
                ("Retained Context".to_string(), Some("read".to_string()))
            }
            "update_project_retained_context" => (
                "Retained Context Update".to_string(),
                Some("edit".to_string()),
            ),
            "delegate_to_worker" => (
                "Delegate To Worker".to_string(),
                Some("execute".to_string()),
            ),
            "delivery_validate_target" => {
                ("Delivery Check".to_string(), Some("search".to_string()))
            }
            "delivery_get_versions" | "delivery_get_latest_version" => {
                ("Delivery Versions".to_string(), Some("search".to_string()))
            }
            "delivery_get_current" => ("Delivery Status".to_string(), Some("search".to_string())),
            "delivery_start" => ("Start Delivery".to_string(), Some("execute".to_string())),
            "delivery_publish" => ("Publish Route".to_string(), Some("execute".to_string())),
            "delivery_merge" => ("Merge Route".to_string(), Some("execute".to_string())),
            "delivery_get_attempts" => {
                ("Delivery Attempts".to_string(), Some("search".to_string()))
            }
            "delivery_retry" => ("Retry Delivery".to_string(), Some("execute".to_string())),
            "delivery_complete" => ("Complete Delivery".to_string(), Some("execute".to_string())),
            "delivery_abandon" => ("Abandon Delivery".to_string(), Some("execute".to_string())),
            "shepherd_get_workers" => ("Workers".to_string(), Some("search".to_string())),
            "shepherd_get_worker_events" => {
                ("Worker Events".to_string(), Some("search".to_string()))
            }
            "shepherd_get_worker_concerns" => {
                ("Worker Concerns".to_string(), Some("search".to_string()))
            }
            "shepherd_resolve_worker_concern" => {
                ("Resolve Concern".to_string(), Some("execute".to_string()))
            }
            _ => (name.to_string(), None),
        }
    }

    fn is_repl_fragment_only(text: &str) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() || !trimmed.contains('<') {
            return false;
        }

        trimmed.chars().all(|c| {
            matches!(
                c.to_ascii_lowercase(),
                '<' | '>' | '/' | 'r' | 'e' | 'p' | 'l' | ' '
            )
        })
    }

    pub(super) fn sanitize_assistant_text(text: &str) -> String {
        let out = text.replace("</repl>", "").replace("<repl>", "");
        if Self::is_repl_fragment_only(&out) {
            String::new()
        } else {
            out
        }
    }
}

fn scope_label(scope: &ShepherdScope) -> String {
    match scope {
        ShepherdScope::General => "general".to_string(),
        ShepherdScope::Project { project_id, .. } => format!("project:{}", project_id),
        ShepherdScope::Effort {
            project_id,
            effort_id,
            ..
        } => format!("effort:{}:{}", project_id, effort_id),
        ShepherdScope::Branch {
            project_id,
            branch_id,
            ..
        } => format!("branch:{}:{}", project_id, branch_id),
    }
}

fn build_scope_guidance(
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    cwd: &Path,
) -> String {
    let focus_line = match focus {
        Some(f) => format!("Focus item: {} ({})", f.task_name, f.task_id),
        None => "Focus item: none".to_string(),
    };

    let scope_header = match scope {
        ShepherdScope::Branch {
            parent_session_id,
            goal,
            ..
        } => format!(
            "## Hirsel Branch Context\n\n\
            This turn is a short-lived private reasoning fork from session `{}`.\n\
            Goal: {}\n\
            {}\n\
            Workspace root: {}\n\n\
            ## Hirsel Constraints\n\n\
            - You are not user-facing. Do not address the user directly.\n\
            - Think through the latest request, inspect Hirsel state, and use tools when needed.\n\
            - You may delegate sandbox work to `code_worker` or `ops_worker` when execution is needed.\n\
            - Return a concise conclusion for the parent channel covering actions taken, current state, recommended reply framing, and any open questions.\n\
            - Do not mention branching, hidden analysis, or internal control flow in the conclusion.\n\
            - Prefer decisions and concrete next actions over long prose.\n",
            parent_session_id,
            goal,
            focus_line,
            cwd.display()
        ),
        ShepherdScope::Effort { title, .. } => format!(
            "## Hirsel Effort\n\n\
            Focused effort: {}\n\
            {}\n\
            Workspace root: {}\n\n\
            ## Hirsel Constraints\n\n\
            - The final assistant response in this scope is shown directly to the user.\n\
            - Stay within this effort unless the user explicitly redirects it.\n\
            - When private branch analysis is supplied in the current turn input, use it as internal context only.\n\
            - Keep updates coherent with the effort's title and current work item.\n\
            - Use Hirsel tools to inspect, plan, mutate work items, and delegate workers when needed.\n\
            - Prefer concrete progress, decisions, and next actions over broad narration.\n\
            - Never mention hidden branching or internal routing.\n",
            title,
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
            - When private branch analysis is supplied in the current turn input, use it as internal context only. Do not mention branching or quote that note verbatim.\n\
            - For route work-tree edits/execution work, use tools; do not edit hidden runtime state directly.\n\
            - When the user wants onboarding, refresh, or understanding of an existing codebase, prefer `sync_project` rather than inventing an ad hoc checklist.\n\
            - For simple conversational questions, answer directly in plain language without REPL code.\n\
            - For work-item content edits, use only `read_work_item` and `apply_patch_work_item`.\n\
            - The project focus view is a maintained artifact. Use `read_project_focus_view` before editing it.\n\
            - Only call `update_project_focus_view` when project meaning materially changed.\n\
            - Focus view updates must replace the full HTML document and preserve stable structure when possible.\n\
            - The focus view is for illustrating the current situation to the user, not reiterating obvious shell context.\n\
            - Do not waste focus-view space repeating the project title, route picker state, or generic chrome the user can already see.\n\
            - Prefer synthesis, comparisons, diagrams, and “what matters now” framing over dashboard filler.\n\
            - Inline Mermaid setup is allowed in the focus view. Do not add arbitrary third-party assets beyond Mermaid.\n\
            - Never claim work happened unless you actually executed tools.\n\
            - Never return raw tool payloads (JSON/Python dict/list) as final user-facing output.\n\
            - Summarize tool outcomes in plain language.\n\
            - For create/setup/scaffold/build/implement requests, perform at least one mutating route work-tree operation before finishing.",
            scope_label(scope),
            focus_line,
            cwd.display()
        ),
    };

    scope_header
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

pub(super) fn looks_like_runtime_traceback(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    t.contains("Traceback (most recent call last):")
        || t.contains("Runtime error:")
        || t.contains("NameError:")
        || t.contains("File \"repl_")
}

pub(super) async fn resolve_scope_project_id(scope: &ShepherdScope) -> Option<i64> {
    match scope {
        ShepherdScope::General => None,
        ShepherdScope::Project { project_id, .. } => Some(*project_id),
        ShepherdScope::Effort { project_id, .. } => Some(*project_id),
        ShepherdScope::Branch { project_id, .. } => Some(*project_id),
    }
}

async fn resolve_route_runtime_workspace(
    project_id: i64,
    route_id: i64,
) -> Result<PathBuf, String> {
    let handle = ensure_route_runtime(project_id, route_id).await?;
    let project_path = handle
        .state
        .get_project_path()
        .await
        .map_err(|error| format!("failed to read runtime workspace path: {}", error))?
        .ok_or_else(|| {
            format!(
                "Route runtime '{}' has no project path recorded",
                handle.runtime_name
            )
        })?;
    let path = PathBuf::from(project_path);
    if !path.exists() || !path.is_dir() {
        return Err(format!(
            "Route runtime '{}' points at missing workspace '{}'",
            handle.runtime_name,
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
            route_id,
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
            resolve_route_runtime_workspace(*project_id, *route_id).await
        }
        ShepherdScope::Effort {
            project_id,
            route_id,
            workspace_path,
            ..
        }
        | ShepherdScope::Branch {
            project_id,
            route_id,
            workspace_path,
            ..
        } => {
            if let Some(path) = workspace_path.as_ref().filter(|p| !p.trim().is_empty()) {
                let path = PathBuf::from(path);
                if path.exists() && path.is_dir() {
                    return Ok(path);
                }
                return Err(format!(
                    "route scope workspace path '{}' does not exist",
                    path.display()
                ));
            }
            resolve_route_runtime_workspace(*project_id, *route_id).await
        }
    }
}
