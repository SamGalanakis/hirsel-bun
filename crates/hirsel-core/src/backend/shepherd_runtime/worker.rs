use std::path::{Path, PathBuf};
use std::sync::Arc;

use lash::tools::StandardShell;
use lash::tools::UpdatePlanTool;
use lash::{
    default_execution_mode, EventSink, ExecutionMode, HostProfile, InputItem, LashRuntime,
    PluginError, PluginFactory, PluginHost, PluginRegistrar, PluginSessionContext,
    PluginSnapshotMeta, PromptContribution, RuntimeHostConfig, RuntimeServices, SessionEvent,
    SessionPlugin, SessionPolicy, SessionStateEnvelope, SnapshotReader, SnapshotWriter,
    ToolProvider, TurnInput,
};
use tokio_util::sync::CancellationToken;

use super::history::{build_runtime_messages, decode_png_images, RUNTIME_HISTORY_LIMIT};
use super::runtime::{
    build_user_turn_text, resolve_scope_project_id, resolve_scope_workspace,
    shepherd_prompt_overrides,
};
use super::shell::ShepherdShellToolProvider;
use super::tools::{LibrarianToolProvider, ShepherdToolProvider};
use super::types::{
    ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus, TurnResult, WorkerStreamEvent,
};
use crate::backend::lash_tools::{
    attach_embedded_mcp_servers, embedded_tool_plugin_factories, EmbeddedCustomToolPlugin,
    EmbeddedToolPreset,
};
use crate::backend::llm_provider;
use crate::backend::prompts;
use crate::backend::ShepherdChatMessage;

const PLAN_TRACKER_GUIDANCE_FALLBACK: &str = "### `update_plan`\nUse `update_plan` for substantial multi-step work. Keep the plan short and concrete, maintain exactly one `in_progress` step, and mark steps completed as soon as they are done.";

fn tool_title_kind(name: &str) -> (String, Option<String>) {
    match name {
        "list_threads" => ("Threads".to_string(), Some("search".to_string())),
        "create_thread" => ("Create Thread".to_string(), Some("execute".to_string())),
        "rename_thread" => ("Rename Thread".to_string(), Some("edit".to_string())),
        "set_thread_status" => ("Thread Status".to_string(), Some("edit".to_string())),
        "archive_thread" => ("Archive Thread".to_string(), Some("execute".to_string())),
        "delete_thread" => ("Delete Thread".to_string(), Some("execute".to_string())),
        "send_thread_message" => ("Message Thread".to_string(), Some("execute".to_string())),
        "read_thread_updates" => ("Thread Updates".to_string(), Some("search".to_string())),
        "forward_port" => ("Forward Port".to_string(), Some("execute".to_string())),
        "list_port_forwards" => ("Port Forwards".to_string(), Some("search".to_string())),
        "close_port_forward" => (
            "Close Port Forward".to_string(),
            Some("execute".to_string()),
        ),
        "read_project_retained_context" => {
            ("Retained Context".to_string(), Some("read".to_string()))
        }
        "update_project_retained_context" => (
            "Retained Context Update".to_string(),
            Some("edit".to_string()),
        ),
        "graph_surql" => (
            "Knowledge Graph Query".to_string(),
            Some("execute".to_string()),
        ),
        "edit_graph_node_text" => (
            "Knowledge Graph Text Patch".to_string(),
            Some("edit".to_string()),
        ),
        "patch_canvas_document" => ("Canvas Patch".to_string(), Some("edit".to_string())),
        "update_plan" => ("Plan Update".to_string(), Some("edit".to_string())),
        _ => (name.to_string(), None),
    }
}

fn sanitize_assistant_text(text: &str) -> String {
    let out = text.replace("</repl>", "").replace("<repl>", "");
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let repl_only = trimmed.contains('<')
        && trimmed.chars().all(|ch| {
            matches!(
                ch.to_ascii_lowercase(),
                '<' | '>' | '/' | 'r' | 'e' | 'p' | 'l' | ' '
            )
        });
    if repl_only {
        String::new()
    } else {
        out
    }
}

fn result_summary_from_chunks(chunks: &[ShepherdMessageChunk]) -> String {
    let joined = chunks
        .iter()
        .filter_map(|chunk| match chunk {
            ShepherdMessageChunk::Text { content } => Some(content.trim()),
            _ => None,
        })
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        return "No summary available yet.".to_string();
    }
    let mut summary = trimmed.chars().take(240).collect::<String>();
    if trimmed.chars().count() > 240 {
        summary.push_str("...");
    }
    summary
}

fn plan_tracker_prompt_contributions() -> Vec<PromptContribution> {
    vec![PromptContribution::guidance(
        "plan_tracker",
        "Plan tracker guidance",
        &prompts::render_plan_tracker_guidance()
            .unwrap_or_else(|_| PLAN_TRACKER_GUIDANCE_FALLBACK.to_string()),
    )]
}

struct EmbeddedPlanTrackerPluginFactory;

impl PluginFactory for EmbeddedPlanTrackerPluginFactory {
    fn id(&self) -> &'static str {
        "plan_tracker"
    }

    fn build(&self, _ctx: &PluginSessionContext) -> Result<Arc<dyn SessionPlugin>, PluginError> {
        Ok(Arc::new(EmbeddedPlanTrackerPlugin {
            tools: Arc::new(UpdatePlanTool::default()),
        }))
    }
}

struct EmbeddedPlanTrackerPlugin {
    tools: Arc<UpdatePlanTool>,
}

impl SessionPlugin for EmbeddedPlanTrackerPlugin {
    fn id(&self) -> &'static str {
        "plan_tracker"
    }

    fn register(&self, reg: &mut PluginRegistrar) -> Result<(), PluginError> {
        reg.tools()
            .provider(Arc::clone(&self.tools) as Arc<dyn ToolProvider>)?;
        reg.prompt().contribute(Arc::new(|_ctx| {
            Box::pin(async move { Ok(plan_tracker_prompt_contributions()) })
        }));
        Ok(())
    }

    fn snapshot(
        &self,
        _writer: &mut dyn SnapshotWriter,
    ) -> Result<PluginSnapshotMeta, PluginError> {
        let snapshot = self
            .tools
            .snapshot()
            .map_err(|err| PluginError::Snapshot(err.to_string()))?;
        Ok(PluginSnapshotMeta {
            plugin_id: self.id().to_string(),
            plugin_version: self.version().to_string(),
            state: Some(
                serde_json::to_value(snapshot)
                    .map_err(|err| PluginError::Snapshot(err.to_string()))?,
            ),
        })
    }

    fn restore(
        &self,
        meta: &PluginSnapshotMeta,
        _reader: &dyn SnapshotReader,
    ) -> Result<(), PluginError> {
        let snapshot = meta
            .state
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|err| PluginError::Snapshot(err.to_string()))?
            .unwrap_or_default();
        self.tools
            .restore(snapshot)
            .map_err(PluginError::Snapshot)?;
        Ok(())
    }
}

async fn build_runtime_services(
    scope: &ShepherdScope,
    default_project_id: Option<i64>,
    workspace_root: Option<PathBuf>,
    agent_id: &str,
    execution_mode: ExecutionMode,
) -> Result<RuntimeServices, String> {
    let tavily_api_key = std::env::var("TAVILY_API_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let shell_tools: Arc<dyn ToolProvider> = match scope {
        ShepherdScope::Shepherd { .. } => Arc::new(ShepherdShellToolProvider::new(
            default_project_id,
            workspace_root.clone(),
        )),
        _ => Arc::new(match workspace_root.as_ref() {
            Some(path) => StandardShell::new().with_cwd(path.clone()),
            None => StandardShell::new(),
        }),
    };
    let (tool_preset, custom_tool_plugin) = match scope {
        ShepherdScope::General => (EmbeddedToolPreset::General, None),
        ShepherdScope::Shepherd { .. } => (
            EmbeddedToolPreset::Shepherd,
            Some(EmbeddedCustomToolPlugin {
                id: "hirsel_shepherd_tools",
                provider: Arc::new(ShepherdToolProvider::new(
                    None,
                    default_project_id,
                    workspace_root.clone(),
                )) as Arc<dyn ToolProvider>,
            }),
        ),
        ShepherdScope::Thread { .. } => (EmbeddedToolPreset::Thread, None),
        ShepherdScope::Librarian { .. } => (
            EmbeddedToolPreset::Librarian,
            Some(EmbeddedCustomToolPlugin {
                id: "hirsel_librarian_tools",
                provider: Arc::new(LibrarianToolProvider::new(
                    None,
                    default_project_id,
                    workspace_root.clone(),
                )) as Arc<dyn ToolProvider>,
            }),
        ),
    };
    let mut plugin_factories = embedded_tool_plugin_factories(
        tool_preset,
        custom_tool_plugin,
        shell_tools,
        tavily_api_key,
    );
    if matches!(scope, ShepherdScope::Shepherd { .. }) {
        plugin_factories.push(Arc::new(EmbeddedPlanTrackerPluginFactory));
    }
    let plugin_host = PluginHost::new(plugin_factories).with_dynamic_tools();
    let root_plugins = plugin_host
        .build_session(agent_id, execution_mode, None)
        .map_err(|e| format!("failed to build shepherd tool session: {}", e))?;
    let dynamic_tools = root_plugins
        .dynamic_tools()
        .ok_or_else(|| "shepherd dynamic tool provider was not initialized".to_string())?;
    let (hirsel_config, _) = crate::backend::config::Config::load()
        .map_err(|e| format!("failed to load config: {}", e))?;
    attach_embedded_mcp_servers(&dynamic_tools, &hirsel_config.mcp_servers).await?;
    Ok(RuntimeServices::new(root_plugins))
}

// ── Direct DB access (replaces RPC calls to server) ──

async fn load_scope_state(scope: &ShepherdScope) -> Result<Option<SessionStateEnvelope>, String> {
    let state_json = super::commands::load_scope_state_local(scope).await?;
    state_json
        .map(|json| {
            serde_json::from_str(&json)
                .map_err(|e| format!("failed to deserialize shepherd scope state: {}", e))
        })
        .transpose()
}

async fn load_scope_messages(
    scope: &ShepherdScope,
    limit: usize,
    skip_message_id: Option<i64>,
) -> Result<Vec<ShepherdChatMessage>, String> {
    super::commands::load_scope_messages_local(scope, limit, skip_message_id).await
}

async fn load_llm_settings() -> Result<crate::backend::app_settings::LlmSettings, String> {
    let store = crate::backend::AppSettingsStore::open()
        .await
        .map_err(|e| format!("failed to open app settings store: {}", e))?;
    store
        .load_llm_settings()
        .await
        .map_err(|e| format!("failed to load llm settings: {}", e))
}

// ── Runtime construction ──

async fn create_runtime_from_history(
    runtime_id: &str,
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    scope_project_id: Option<i64>,
    cwd: &Path,
    history: &[ShepherdChatMessage],
) -> Result<LashRuntime, String> {
    let settings = load_llm_settings().await?;
    let provider = llm_provider::resolve_provider(&settings).await?;
    let role = match scope {
        ShepherdScope::Shepherd { .. } => llm_provider::RuntimeModelRole::Shepherd,
        ShepherdScope::Thread { .. } => llm_provider::RuntimeModelRole::Thread,
        ShepherdScope::Librarian { .. } => llm_provider::RuntimeModelRole::Librarian,
        ShepherdScope::General => llm_provider::RuntimeModelRole::Shepherd,
    };
    let (model, model_variant) = llm_provider::resolve_model_for_role(&settings, &provider, role);
    let execution_mode = default_execution_mode();
    let session_policy = SessionPolicy {
        model: model.clone(),
        provider,
        max_context_tokens: Some(crate::backend::config::get_context_window(&model) as usize),
        model_variant,
        session_id: Some(runtime_id.to_string()),
        execution_mode,
        ..Default::default()
    };
    let host_config = RuntimeHostConfig {
        host_profile: HostProfile::Embedded,
        base_dir: Some(cwd.to_path_buf()),
        prompt_overrides: shepherd_prompt_overrides(scope, focus, cwd).await,
        ..RuntimeHostConfig::default()
    };
    let state = SessionStateEnvelope {
        session_id: format!("shepherd-{}", runtime_id),
        policy: session_policy.clone(),
        messages: build_runtime_messages(history),
        ..SessionStateEnvelope::default()
    };
    let services = build_runtime_services(
        scope,
        scope_project_id,
        Some(cwd.to_path_buf()),
        &state.session_id,
        session_policy.execution_mode,
    )
    .await?;

    LashRuntime::from_state(session_policy, host_config, services, state)
        .await
        .map_err(|e| format!("failed to create shepherd lash runtime: {}", e))
}

async fn create_runtime_from_state(
    runtime_id: &str,
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    scope_project_id: Option<i64>,
    cwd: &Path,
    mut state: SessionStateEnvelope,
) -> Result<LashRuntime, String> {
    state.session_id = format!("shepherd-{}", runtime_id);
    state.policy.session_id = Some(runtime_id.to_string());
    let session_policy = state.policy.clone();
    let host_config = RuntimeHostConfig {
        host_profile: HostProfile::Embedded,
        base_dir: Some(cwd.to_path_buf()),
        prompt_overrides: shepherd_prompt_overrides(scope, focus, cwd).await,
        ..RuntimeHostConfig::default()
    };
    let services = build_runtime_services(
        scope,
        scope_project_id,
        Some(cwd.to_path_buf()),
        &state.session_id,
        session_policy.execution_mode,
    )
    .await?;

    LashRuntime::from_state(session_policy, host_config, services, state)
        .await
        .map_err(|e| format!("failed to create shepherd runtime: {}", e))
}

// ── Channel-based event sink (replaces StreamingRpcSink) ──

struct ChannelEventSink {
    tx: tokio::sync::mpsc::Sender<WorkerStreamEvent>,
    snapshot_template: SessionStateEnvelope,
}

#[async_trait::async_trait]
impl EventSink for ChannelEventSink {
    async fn emit(&self, event: SessionEvent) {
        let stream_event = match event {
            SessionEvent::TextDelta { content } => WorkerStreamEvent::TextDelta { content },
            SessionEvent::DurableSnapshot { snapshot } => {
                let mut state = self.snapshot_template.clone();
                state.messages = snapshot.messages;
                state.tool_calls = snapshot.tool_calls;
                state.iteration = snapshot.iteration;
                match serde_json::to_string(&state) {
                    Ok(state_json) => WorkerStreamEvent::DurableSnapshot { state_json },
                    Err(error) => WorkerStreamEvent::Error {
                        message: format!("failed to serialize durable snapshot: {}", error),
                    },
                }
            }
            SessionEvent::ToolCall {
                call_id,
                name,
                args,
                result,
                success,
                ..
            } => {
                let (title, kind) = tool_title_kind(&name);
                WorkerStreamEvent::Tool {
                    id: call_id.unwrap_or_else(|| name.clone()),
                    title,
                    kind,
                    status: if success {
                        "completed".to_string()
                    } else {
                        "failed".to_string()
                    },
                    input: serde_json::to_string(&args).ok(),
                    output: serde_json::to_string(&result).ok(),
                }
            }
            SessionEvent::Message { text, kind } => WorkerStreamEvent::Message { text, kind },
            SessionEvent::Error { message, .. } => WorkerStreamEvent::Error { message },
            _ => return,
        };
        let _ = self.tx.send(stream_event).await;
    }
}

// ── Main in-process turn execution ──

pub async fn run_scope_turn(
    scope: &ShepherdScope,
    user_chunks: Vec<ShepherdMessageChunk>,
    focus: Option<ShepherdTaskFocus>,
    user_message_id: Option<i64>,
    event_tx: tokio::sync::mpsc::Sender<WorkerStreamEvent>,
    cancel: CancellationToken,
) -> Result<TurnResult, String> {
    let focus = focus.or_else(|| match scope {
        ShepherdScope::Shepherd { focus, .. } | ShepherdScope::Thread { focus, .. } => {
            focus.clone()
        }
        _ => None,
    });
    let user_images_png = decode_png_images(&user_chunks)?;
    let user_turn_text = build_user_turn_text(scope, &user_chunks).await?;
    let history = load_scope_messages(scope, RUNTIME_HISTORY_LIMIT, user_message_id).await?;
    let cwd = resolve_scope_workspace(scope).await?;
    let scope_project_id = resolve_scope_project_id(scope).await;

    let runtime = if let Some(state) = load_scope_state(scope).await? {
        create_runtime_from_state(
            "worker-session",
            scope,
            focus.as_ref(),
            scope_project_id,
            &cwd,
            state,
        )
        .await?
    } else {
        create_runtime_from_history(
            "worker-session",
            scope,
            focus.as_ref(),
            scope_project_id,
            &cwd,
            &history,
        )
        .await?
    };

    let mut turn_items = Vec::new();
    turn_items.push(InputItem::Text {
        text: user_turn_text,
    });
    let mut image_blobs = std::collections::HashMap::new();
    for (idx, bytes) in user_images_png.into_iter().enumerate() {
        let id = format!("image-{}", idx + 1);
        turn_items.push(InputItem::ImageRef { id: id.clone() });
        image_blobs.insert(id, bytes);
    }

    let mut runtime = runtime;
    let sink = ChannelEventSink {
        tx: event_tx,
        snapshot_template: runtime.export_state(),
    };
    let turn = runtime
        .stream_turn(
            TurnInput {
                items: turn_items,
                image_blobs,
                mode: None,
                user_input: None,
            },
            &sink,
            cancel.clone(),
        )
        .await
        .map_err(|e| format!("failed to run shepherd turn: {}", e))?;

    let final_text = sanitize_assistant_text(&turn.assistant_output.safe_text);
    let mut assistant_chunks = Vec::new();
    if !final_text.trim().is_empty() {
        assistant_chunks.push(ShepherdMessageChunk::Text {
            content: final_text,
        });
    }
    assistant_chunks.extend(turn.tool_calls.iter().enumerate().map(|(idx, record)| {
        let (title, kind) = tool_title_kind(&record.tool);
        ShepherdMessageChunk::Tool {
            id: record
                .call_id
                .clone()
                .unwrap_or_else(|| format!("tool-{}", idx + 1)),
            title,
            kind,
            status: if record.success {
                "completed".to_string()
            } else {
                "failed".to_string()
            },
            input: serde_json::to_string(&record.args).ok(),
            output: serde_json::to_string(&record.result).ok(),
        }
    }));

    if assistant_chunks.is_empty() {
        tracing::warn!(scope = ?scope, "model turn completed without visible output");
        return Err("Model returned no usable output.".to_string());
    }

    let state_json = serde_json::to_string(&runtime.export_state())
        .map_err(|e| format!("failed to serialize shepherd scope state: {}", e))?;
    let summary = result_summary_from_chunks(&assistant_chunks);
    let interrupted = cancel.is_cancelled();

    Ok(TurnResult {
        assistant_chunks,
        state_json,
        summary,
        interrupted,
    })
}
