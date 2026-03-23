use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use lash::provider::Provider;
use lash::{AgentEvent, EventSink, PromptOverrideMode, PromptSectionName, PromptSectionOverride};
use serde::Serialize;
use tauri::Emitter;
use tokio::sync::Mutex;
use tracing::warn;

use super::history::{chunk_image_count, chunk_text};
use super::types::{
    ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus, StartShepherdSessionRequest,
};
use crate::core::llm_provider;
use crate::core::{Config, ProjectStore, RouteFiles, RouteStore};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(super) enum ShepherdEvent {
    TextDelta {
        session_id: String,
        text: String,
    },
    ToolCallStart {
        session_id: String,
        tool_call_id: String,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        input: Option<String>,
    },
    ToolCallUpdate {
        session_id: String,
        tool_call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    Error {
        session_id: String,
        message: String,
    },
    MessageComplete {
        session_id: String,
    },
    SessionEnded {
        session_id: String,
    },
}

#[derive(Default)]
pub(super) struct AssistantDraft {
    pub text: String,
    pub thinking: String,
    pub tools: Vec<ShepherdMessageChunk>,
    pub runtime_output: String,
    pub errored: bool,
}

pub(super) struct ShepherdLashSink {
    app: tauri::AppHandle,
    session_id: String,
    tool_seq: AtomicU64,
    draft: Arc<Mutex<AssistantDraft>>,
}

impl ShepherdLashSink {
    pub(super) fn new(
        app: tauri::AppHandle,
        session_id: String,
        draft: Arc<Mutex<AssistantDraft>>,
    ) -> Self {
        Self {
            app,
            session_id,
            draft,
            tool_seq: AtomicU64::new(1),
        }
    }

    fn emit(&self, event: &ShepherdEvent) {
        if let Err(e) = self.app.emit("shepherd-event", (&self.session_id, event)) {
            warn!("failed to emit shepherd-event: {}", e);
        }
    }

    fn tool_title_kind(name: &str) -> (String, Option<String>) {
        match name {
            "list_routes" => ("Routes".to_string(), Some("search".to_string())),
            "fork_route" => ("Fork Route".to_string(), Some("execute".to_string())),
            "select_route" => ("Select Route".to_string(), Some("execute".to_string())),
            "archive_route" => ("Archive Route".to_string(), Some("execute".to_string())),
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

#[async_trait::async_trait]
impl EventSink for ShepherdLashSink {
    async fn emit(&self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta { content } => {
                let sanitized = Self::sanitize_assistant_text(&content);
                if sanitized.is_empty() {
                    return;
                }
                let mut draft = self.draft.lock().await;
                draft.text.push_str(&sanitized);
            }
            AgentEvent::CodeBlock { code } => {
                let _ = code;
            }
            AgentEvent::CodeOutput { output, error } => {
                let mut draft = self.draft.lock().await;
                if !output.trim().is_empty() {
                    if !draft.runtime_output.is_empty() {
                        draft.runtime_output.push('\n');
                    }
                    draft.runtime_output.push_str(&output);
                }
                if let Some(err) = error {
                    if !err.trim().is_empty() {
                        if !draft.runtime_output.is_empty() {
                            draft.runtime_output.push('\n');
                        }
                        draft
                            .runtime_output
                            .push_str(&format!("Runtime error: {}", err));
                    }
                }
            }
            AgentEvent::ToolCall {
                name,
                args,
                result,
                success,
                ..
            } => {
                let tool_call_id = format!(
                    "shepherd-tool-{}",
                    self.tool_seq.fetch_add(1, Ordering::Relaxed)
                );
                let (title, kind) = Self::tool_title_kind(&name);
                let input = serde_json::to_string(&args).ok();
                let output = serde_json::to_string(&result).ok();

                self.emit(&ShepherdEvent::ToolCallStart {
                    session_id: self.session_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    title: title.clone(),
                    kind: kind.clone(),
                    input: input.clone(),
                });

                let status = if success { "completed" } else { "failed" }.to_string();
                self.emit(&ShepherdEvent::ToolCallUpdate {
                    session_id: self.session_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    title: Some(title.clone()),
                    status: status.clone(),
                    output: output.clone(),
                });

                let mut draft = self.draft.lock().await;
                draft.tools.push(ShepherdMessageChunk::Tool {
                    id: tool_call_id,
                    title,
                    kind,
                    status,
                    input,
                    output,
                });
            }
            AgentEvent::Message { text, kind } => {
                if kind == "final" {
                    let sanitized_final = Self::sanitize_assistant_text(&text);
                    let mut draft = self.draft.lock().await;
                    if draft.text.trim().is_empty() && !sanitized_final.trim().is_empty() {
                        draft.text.push_str(sanitized_final.trim());
                    }
                }
            }
            AgentEvent::Error { message, .. } => {
                self.emit(&ShepherdEvent::Error {
                    session_id: self.session_id.clone(),
                    message: message.clone(),
                });
                let mut draft = self.draft.lock().await;
                draft.errored = true;
            }
            AgentEvent::Prompt { .. }
            | AgentEvent::LlmRequest { .. }
            | AgentEvent::LlmResponse { .. }
            | AgentEvent::TokenUsage { .. }
            | AgentEvent::RetryStatus { .. }
            | AgentEvent::InjectedMessagesCommitted { .. }
            | AgentEvent::PluginEvent { .. }
            | AgentEvent::Done => {}
        }
    }
}

pub(super) fn build_scope(request: StartShepherdSessionRequest) -> ShepherdScope {
    match request {
        StartShepherdSessionRequest::General => ShepherdScope::General,
        StartShepherdSessionRequest::Project { project_id } => ShepherdScope::Project {
            project_id,
            workspace_path: None,
            focus: None,
        },
        StartShepherdSessionRequest::ProjectFocused {
            project_id,
            task_id,
            task_name,
        } => ShepherdScope::Project {
            project_id,
            workspace_path: None,
            focus: Some(ShepherdTaskFocus { task_id, task_name }),
        },
    }
}

fn scope_label(scope: &ShepherdScope) -> String {
    match scope {
        ShepherdScope::General => "general".to_string(),
        ShepherdScope::Project { project_id, .. } => format!("project:{}", project_id),
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

    format!(
        "## Hirsel Shepherd Scope\n\n\
        Scope: {}\n\
        {}\n\
        Workspace root: {}\n\n\
        ## Hirsel Constraints\n\n\
        - For route work-tree edits/execution work, use tools; do not edit hidden runtime state directly.\n\
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
    )
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

pub(super) fn build_assistant_chunks(
    draft: &AssistantDraft,
    final_text: &str,
) -> Vec<ShepherdMessageChunk> {
    let mut chunks = Vec::new();

    if !draft.thinking.trim().is_empty() {
        chunks.push(ShepherdMessageChunk::Thinking {
            content: draft.thinking.clone(),
        });
    }

    let sanitized_text = ShepherdLashSink::sanitize_assistant_text(final_text);
    if !sanitized_text.trim().is_empty() {
        chunks.push(ShepherdMessageChunk::Text {
            content: sanitized_text,
        });
    }

    chunks.extend(draft.tools.clone());
    chunks
}

pub(super) async fn resolve_scope_project_id(scope: &ShepherdScope) -> Option<i64> {
    match scope {
        ShepherdScope::General => None,
        ShepherdScope::Project { project_id, .. } => Some(*project_id),
    }
}

pub(super) async fn resolve_scope_workspace(scope: &ShepherdScope) -> Option<PathBuf> {
    match scope {
        ShepherdScope::General => None,
        ShepherdScope::Project {
            project_id,
            workspace_path,
            ..
        } => {
            if let Some(path) = workspace_path.as_ref().filter(|p| !p.trim().is_empty()) {
                return Some(PathBuf::from(path));
            }

            let project_store = ProjectStore::open().await.ok()?;
            let project = project_store.get_project(*project_id).await.ok()?;
            let route_store = RouteStore::new(*project_id).await.ok()?;

            let route = if let Some(route_id) = project.active_route_id {
                match route_store.get_route(route_id).await {
                    Ok(route) => route,
                    Err(_) => route_store.create_main_route().await.ok()?,
                }
            } else {
                route_store.create_main_route().await.ok()?
            };

            Some(RouteFiles::new(*project_id, &route.name).route_dir())
        }
    }
}

pub(super) fn resolve_runtime_cwd(path: Option<PathBuf>) -> PathBuf {
    if let Some(path) = path.filter(|p| p.exists() && p.is_dir()) {
        path
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }
}

pub(super) async fn load_shepherd_provider() -> Result<Provider, String> {
    let (config, _) = Config::load().map_err(|e| format!("failed to load config: {}", e))?;
    llm_provider::resolve_provider(&config).await
}
