use std::path::{Path, PathBuf};
use std::sync::Arc;

use lash::tools::UpdatePlanTool;
use lash::{
    default_context_strategy, default_execution_mode, AgentEvent, AgentStateEnvelope, EventSink,
    ExecutionMode, HostProfile, InputItem, LashRuntime, PluginError, PluginFactory, PluginHost,
    PluginRegistrar, PluginSessionContext, PluginSnapshotMeta, PromptContribution,
    RuntimeHostConfig, RuntimeServices, SessionPlugin, SessionPolicy, SnapshotReader,
    SnapshotWriter, ToolProvider, TurnInput,
};
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use super::history::{build_runtime_messages, decode_png_images, RUNTIME_HISTORY_LIMIT};
use super::rpc::{read_json_line, write_json_line, WorkerReply, WorkerRequest, WorkerStreamEvent};
use super::runtime::{
    build_user_turn_text, resolve_scope_project_id, resolve_scope_workspace,
    shepherd_prompt_overrides,
};
use super::tools::ShepherdToolProvider;
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
use crate::backend::app::ResultExt;
use crate::backend::credentials::CredentialStore;
use crate::backend::lash_tools::{attach_embedded_mcp_servers, embedded_tool_plugin_factories};
use crate::backend::llm_provider;
use crate::backend::{ShepherdChatMessage, ShepherdChatStore, ShepherdThreadStore};

fn scope_storage_ids(scope: &ShepherdScope) -> (Option<i64>, Option<String>) {
    match scope {
        ShepherdScope::General => (None, None),
        ShepherdScope::Project { project_id, .. } => (
            Some(*project_id),
            Some(ShepherdChatStore::project_scope_key(*project_id)),
        ),
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => (
            Some(*project_id),
            Some(ShepherdThreadStore::scope_key(thread_id)),
        ),
    }
}

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
        "read_canvas" => ("Canvas".to_string(), Some("read".to_string())),
        "update_canvas" => ("Canvas Update".to_string(), Some("edit".to_string())),
        "read_project_retained_context" => {
            ("Retained Context".to_string(), Some("read".to_string()))
        }
        "update_project_retained_context" => (
            "Retained Context Update".to_string(),
            Some("edit".to_string()),
        ),
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

async fn load_tavily_api_key() -> Option<String> {
    match CredentialStore::open().await {
        Ok(store) => store.load("tavily_api_key").await.ok(),
        Err(_) => None,
    }
}

fn plan_tracker_prompt_contributions() -> Vec<PromptContribution> {
    vec![PromptContribution::guidance(
        "### `update_plan`\nUse `update_plan` for substantial multi-step work. Keep the plan short and concrete, maintain exactly one `in_progress` step, and mark steps completed as soon as they are done.",
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
    default_project_id: Option<i64>,
    workspace_root: Option<PathBuf>,
    agent_id: &str,
    execution_mode: ExecutionMode,
) -> Result<RuntimeServices, String> {
    let tools: Arc<dyn ToolProvider> = Arc::new(ShepherdToolProvider::new(
        None,
        default_project_id,
        workspace_root,
    ));
    let mut plugin_factories = embedded_tool_plugin_factories(
        "hirsel_shepherd_tools",
        Arc::clone(&tools),
        load_tavily_api_key().await,
    );
    plugin_factories.push(Arc::new(EmbeddedPlanTrackerPluginFactory));
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

async fn load_scope_state(scope: &ShepherdScope) -> Result<Option<AgentStateEnvelope>, String> {
    let (Some(project_id), Some(scope_key)) = scope_storage_ids(scope) else {
        return Ok(None);
    };
    let store = ShepherdChatStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd chat store: {}", e))?;
    let Some(state_json) = store
        .get_scope_state(project_id, &scope_key)
        .await
        .map_err(|e| format!("failed to load shepherd scope state: {}", e))?
    else {
        return Ok(None);
    };

    serde_json::from_str(&state_json)
        .map(Some)
        .map_err(|e| format!("failed to deserialize shepherd scope state: {}", e))
}

async fn load_scope_messages(
    scope: &ShepherdScope,
    limit: usize,
    skip_message_id: Option<i64>,
) -> Result<Vec<ShepherdChatMessage>, String> {
    let store = ShepherdChatStore::open().await.str_err()?;
    let mut messages = match scope {
        ShepherdScope::General => store.get_messages(None).await.str_err()?,
        ShepherdScope::Project { project_id, .. } => store
            .get_scope_messages(
                Some(*project_id),
                Some(&ShepherdChatStore::project_scope_key(*project_id)),
                limit,
            )
            .await
            .str_err()?,
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => store
            .get_scope_messages(
                Some(*project_id),
                Some(&ShepherdChatStore::thread_scope_key(thread_id)),
                limit,
            )
            .await
            .str_err()?,
    };
    if let Some(skip_id) = skip_message_id {
        messages.retain(|message| message.id != skip_id);
    }
    Ok(messages)
}

async fn create_runtime_from_history(
    runtime_id: &str,
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    scope_project_id: Option<i64>,
    cwd: &Path,
    history: &[ShepherdChatMessage],
) -> Result<LashRuntime, String> {
    let (hirsel_config, _) = crate::backend::config::Config::load()
        .map_err(|e| format!("failed to load config: {}", e))?;
    let provider = llm_provider::resolve_provider(&hirsel_config).await?;
    let (model, model_variant) = llm_provider::resolve_model(&hirsel_config, &provider);
    let execution_mode = default_execution_mode();
    let context_strategy = default_context_strategy();
    let session_policy = SessionPolicy {
        model: model.clone(),
        provider,
        max_context_tokens: Some(crate::backend::config::get_context_window(&model) as usize),
        model_variant,
        session_id: Some(runtime_id.to_string()),
        execution_mode,
        context_strategy,
        ..Default::default()
    };
    let host_config = RuntimeHostConfig {
        host_profile: HostProfile::Embedded,
        base_dir: Some(cwd.to_path_buf()),
        prompt_overrides: shepherd_prompt_overrides(scope, focus, cwd),
        ..RuntimeHostConfig::default()
    };
    let state = AgentStateEnvelope {
        agent_id: format!("shepherd-{}", runtime_id),
        policy: session_policy.clone(),
        messages: build_runtime_messages(history),
        ..AgentStateEnvelope::default()
    };
    let services = build_runtime_services(
        scope_project_id,
        Some(cwd.to_path_buf()),
        &state.agent_id,
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
    mut state: AgentStateEnvelope,
) -> Result<LashRuntime, String> {
    state.agent_id = format!("shepherd-{}", runtime_id);
    state.policy.session_id = Some(runtime_id.to_string());
    let session_policy = state.policy.clone();
    let host_config = RuntimeHostConfig {
        host_profile: HostProfile::Embedded,
        base_dir: Some(cwd.to_path_buf()),
        prompt_overrides: shepherd_prompt_overrides(scope, focus, cwd),
        ..RuntimeHostConfig::default()
    };
    let services = build_runtime_services(
        scope_project_id,
        Some(cwd.to_path_buf()),
        &state.agent_id,
        session_policy.execution_mode,
    )
    .await?;

    LashRuntime::from_state(session_policy, host_config, services, state)
        .await
        .map_err(|e| format!("failed to create shepherd scope runtime: {}", e))
}

async fn emit_stream_event(
    writer: &Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    event: WorkerStreamEvent,
) {
    let mut guard = writer.lock().await;
    let _ = write_json_line(&mut *guard, &WorkerReply::Event { event }).await;
}

struct StreamingRpcSink {
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
}

#[async_trait::async_trait]
impl EventSink for StreamingRpcSink {
    async fn emit(&self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta { content } => {
                emit_stream_event(&self.writer, WorkerStreamEvent::TextDelta { content }).await;
            }
            AgentEvent::ToolCall {
                call_id,
                name,
                args,
                result,
                success,
                ..
            } => {
                let (title, kind) = tool_title_kind(&name);
                emit_stream_event(
                    &self.writer,
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
                    },
                )
                .await;
            }
            AgentEvent::Message { text, kind } => {
                emit_stream_event(&self.writer, WorkerStreamEvent::Message { text, kind }).await;
            }
            AgentEvent::Error { message, .. } => {
                emit_stream_event(&self.writer, WorkerStreamEvent::Error { message }).await;
            }
            AgentEvent::DurableSnapshot { .. } => {}
            _ => {}
        }
    }
}

async fn run_scope_turn(
    scope: &ShepherdScope,
    user_chunks: Vec<ShepherdMessageChunk>,
    focus: Option<ShepherdTaskFocus>,
    user_message_id: Option<i64>,
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    cancel: CancellationToken,
) -> Result<(Vec<ShepherdMessageChunk>, String, String), String> {
    let focus = focus.or_else(|| match scope {
        ShepherdScope::Project { focus, .. } | ShepherdScope::Thread { focus, .. } => focus.clone(),
        _ => None,
    });
    let user_images_png = decode_png_images(&user_chunks)?;
    let user_turn_text = build_user_turn_text(&user_chunks);
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
    let sink = StreamingRpcSink { writer };
    let turn = runtime
        .stream_turn(
            TurnInput {
                items: turn_items,
                image_blobs,
                mode: None,
            },
            &sink,
            cancel.clone(),
        )
        .await
        .map_err(|e| format!("failed to run shepherd turn: {}", e))?;

    if cancel.is_cancelled() {
        return Err("Turn interrupted.".to_string());
    }

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
        return Err("Shepherd returned no user-visible output for this turn.".to_string());
    }

    let state_json = serde_json::to_string(&runtime.export_state())
        .map_err(|e| format!("failed to serialize shepherd scope state: {}", e))?;
    let summary = result_summary_from_chunks(&assistant_chunks);
    Ok((assistant_chunks, state_json, summary))
}

#[derive(Clone, Default)]
struct WorkerControl {
    status: Arc<RwLock<String>>,
    cancel: Arc<Mutex<Option<CancellationToken>>>,
}

impl WorkerControl {
    async fn status(&self) -> String {
        self.status.read().await.clone()
    }

    async fn set_status(&self, value: &str) {
        *self.status.write().await = value.to_string();
    }
}

async fn handle_connection(
    scope: ShepherdScope,
    control: WorkerControl,
    stream: UnixStream,
) -> Result<(), String> {
    let (read_half, write_half) = stream.into_split();
    let writer = Arc::new(Mutex::new(write_half));
    let mut reader = BufReader::new(read_half);
    let request: WorkerRequest = read_json_line(&mut reader).await?;
    match request {
        WorkerRequest::Ping => {
            let mut guard = writer.lock().await;
            write_json_line(&mut *guard, &WorkerReply::Pong).await?;
        }
        WorkerRequest::Status => {
            let status = control.status().await;
            let mut guard = writer.lock().await;
            write_json_line(&mut *guard, &WorkerReply::Status { status }).await?;
        }
        WorkerRequest::Interrupt => {
            if let Some(token) = control.cancel.lock().await.as_ref() {
                token.cancel();
            }
            let mut guard = writer.lock().await;
            write_json_line(
                &mut *guard,
                &WorkerReply::Status {
                    status: "interrupting".to_string(),
                },
            )
            .await?;
        }
        WorkerRequest::RunTurn {
            user_chunks,
            focus,
            user_message_id,
        } => {
            {
                let mut cancel = control.cancel.lock().await;
                if cancel.is_some() {
                    let mut guard = writer.lock().await;
                    write_json_line(
                        &mut *guard,
                        &WorkerReply::Error {
                            message: "This worker is already running a turn.".to_string(),
                        },
                    )
                    .await?;
                    return Ok(());
                }
                *cancel = Some(CancellationToken::new());
            }
            control.set_status("running").await;
            {
                let mut guard = writer.lock().await;
                write_json_line(&mut *guard, &WorkerReply::Accepted).await?;
            }
            let cancel = control
                .cancel
                .lock()
                .await
                .as_ref()
                .cloned()
                .expect("cancel token should exist");
            let result = run_scope_turn(
                &scope,
                user_chunks,
                focus,
                user_message_id,
                Arc::clone(&writer),
                cancel,
            )
            .await;
            *control.cancel.lock().await = None;
            match result {
                Ok((assistant_chunks, state_json, summary)) => {
                    control.set_status("idle").await;
                    let mut guard = writer.lock().await;
                    write_json_line(
                        &mut *guard,
                        &WorkerReply::Finished {
                            assistant_chunks,
                            state_json,
                            summary,
                        },
                    )
                    .await?;
                }
                Err(error) => {
                    control.set_status("failed").await;
                    let mut guard = writer.lock().await;
                    write_json_line(&mut *guard, &WorkerReply::Error { message: error }).await?;
                }
            }
        }
    }
    Ok(())
}

pub async fn serve_worker_session(
    scope_file: &Path,
    socket_path: &Path,
    _bootstrap_flake: bool,
) -> Result<(), String> {
    let bytes = tokio::fs::read(scope_file)
        .await
        .map_err(|error| format!("failed to read scope file: {}", error))?;
    let scope: ShepherdScope = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid shepherd scope json: {}", error))?;

    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("failed to create worker socket dir: {}", error))?;
    }
    if socket_path.exists() {
        let _ = tokio::fs::remove_file(socket_path).await;
    }

    let listener = UnixListener::bind(socket_path)
        .map_err(|error| format!("failed to bind worker socket: {}", error))?;
    let control = WorkerControl::default();
    control.set_status("idle").await;

    loop {
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|error| format!("failed to accept worker socket connection: {}", error))?;
        let scope = scope.clone();
        let control = control.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(scope, control, stream).await {
                tracing::warn!(%error, "worker rpc request failed");
            }
        });
    }
}
