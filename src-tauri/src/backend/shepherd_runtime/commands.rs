use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use lash::tools::UpdatePlanTool;
use lash::{
    default_context_strategy, default_execution_mode, AgentStateEnvelope, EventSink, ExecutionMode,
    HostProfile, InputItem, LashRuntime, PluginError, PluginFactory, PluginHost, PluginRegistrar,
    PluginSessionContext, PluginSnapshotMeta, PromptContribution, RuntimeHostConfig,
    RuntimeServices, SessionPlugin, SessionPolicy, SnapshotReader, SnapshotWriter, ToolProvider,
    TurnInput,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::history::{
    build_runtime_messages, build_user_chunks, chunks_to_json, decode_png_images,
    load_scope_messages, load_scope_queue, parse_chunks_from_json, save_message,
    RUNTIME_HISTORY_LIMIT,
};
use super::runtime::{
    build_user_turn_text, resolve_scope_project_id, resolve_scope_workspace,
    shepherd_prompt_overrides, ShepherdLashSink,
};
use super::tools::ShepherdToolProvider;
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
use crate::backend::credentials::CredentialStore;
use crate::backend::lash_tools::{attach_embedded_mcp_servers, embedded_tool_plugin_factories};
use crate::backend::shepherd_threads::prepare_thread_workspace;
use crate::backend::{
    llm_provider, ProjectStore, Route, RouteStore, ShepherdChatMessage, ShepherdChatStore,
    ShepherdQueuedTurn, ShepherdThread, ShepherdThreadStore,
};

pub(super) struct SilentLashSink;
const PROJECT_SURVEY_THREAD_TITLE: &str = "Project survey";

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

static ACTIVE_SCOPE_PROCESSORS: OnceLock<StdMutex<HashSet<String>>> = OnceLock::new();

fn active_scope_processors() -> &'static StdMutex<HashSet<String>> {
    ACTIVE_SCOPE_PROCESSORS.get_or_init(|| StdMutex::new(HashSet::new()))
}

#[async_trait::async_trait]
impl EventSink for SilentLashSink {
    async fn emit(&self, _event: lash::AgentEvent) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueShepherdMessageResponse {
    pub queued: bool,
    pub queue_depth: usize,
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdQueueState {
    pub items: Vec<ShepherdQueuedTurn>,
    pub has_active_turn: bool,
}

async fn load_tavily_api_key() -> Option<String> {
    match CredentialStore::open().await {
        Ok(store) => store.load("tavily_api_key").await.ok(),
        Err(_) => None,
    }
}

async fn build_runtime_services(
    app: Option<&tauri::AppHandle>,
    default_project_id: Option<i64>,
    workspace_root: Option<std::path::PathBuf>,
    agent_id: &str,
    execution_mode: ExecutionMode,
) -> Result<RuntimeServices, String> {
    let tools: Arc<dyn ToolProvider> = Arc::new(ShepherdToolProvider::new(
        app.cloned(),
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

async fn create_runtime_from_history(
    app: Option<&tauri::AppHandle>,
    runtime_id: &str,
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    scope_project_id: Option<i64>,
    cwd: &std::path::Path,
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
        app,
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

async fn resolve_target_route(project_id: i64, route_id: Option<i64>) -> Result<Route, String> {
    let route_store = RouteStore::new(project_id)
        .await
        .map_err(|e| format!("failed to open route store: {}", e))?;

    if let Some(route_id) = route_id {
        return route_store
            .get_route(route_id)
            .await
            .map_err(|e| format!("failed to load route {}: {}", route_id, e));
    }

    let project_store = ProjectStore::open()
        .await
        .map_err(|e| format!("failed to open project store: {}", e))?;
    let project = project_store
        .get_project(project_id)
        .await
        .map_err(|e| format!("failed to load project {}: {}", project_id, e))?;

    if let Some(active_route_id) = project.active_route_id {
        if let Ok(route) = route_store.get_route(active_route_id).await {
            return Ok(route);
        }
    }

    route_store
        .list_routes()
        .await
        .map_err(|e| format!("failed to list routes: {}", e))?
        .into_iter()
        .next()
        .ok_or_else(|| "No routes exist for this project".to_string())
}

fn scope_queue_key(scope: &ShepherdScope) -> Option<String> {
    match scope {
        ShepherdScope::General => Some("general".to_string()),
        ShepherdScope::Project { project_id, .. } => Some(format!("project:{}", project_id)),
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => Some(format!("thread:{}:{}", project_id, thread_id)),
    }
}

fn scope_storage_ids(scope: &ShepherdScope) -> (Option<i64>, Option<String>) {
    match scope {
        ShepherdScope::General => (None, None),
        ShepherdScope::Project {
            project_id,
            route_id,
            ..
        } => (
            Some(*project_id),
            Some(ShepherdChatStore::project_runtime_name(*route_id)),
        ),
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => (
            Some(*project_id),
            Some(ShepherdThreadStore::runtime_name(thread_id)),
        ),
    }
}

async fn load_scope_state(scope: &ShepherdScope) -> Result<Option<AgentStateEnvelope>, String> {
    let (Some(project_id), Some(runtime_name)) = scope_storage_ids(scope) else {
        return Ok(None);
    };
    let store = ShepherdChatStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd chat store: {}", e))?;
    let Some(state_json) = store
        .get_scope_state(project_id, &runtime_name)
        .await
        .map_err(|e| format!("failed to load shepherd scope state: {}", e))?
    else {
        return Ok(None);
    };

    serde_json::from_str(&state_json)
        .map(Some)
        .map_err(|e| format!("failed to deserialize shepherd scope state: {}", e))
}

async fn save_scope_state(scope: &ShepherdScope, state: &AgentStateEnvelope) -> Result<(), String> {
    let (Some(project_id), Some(runtime_name)) = scope_storage_ids(scope) else {
        return Ok(());
    };
    let store = ShepherdChatStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd chat store: {}", e))?;
    let state_json = serde_json::to_string(state)
        .map_err(|e| format!("failed to serialize shepherd scope state: {}", e))?;
    store
        .save_scope_state(project_id, &runtime_name, &state_json)
        .await
        .map_err(|e| format!("failed to save shepherd scope state: {}", e))
}

async fn create_runtime_from_state(
    app: Option<&tauri::AppHandle>,
    runtime_id: &str,
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    scope_project_id: Option<i64>,
    cwd: &std::path::Path,
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
        app,
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

fn tool_chunks_from_records(tool_calls: &[lash::ToolCallRecord]) -> Vec<ShepherdMessageChunk> {
    tool_calls
        .iter()
        .enumerate()
        .map(|(idx, record)| {
            let (title, kind) = ShepherdLashSink::tool_title_kind(&record.tool);
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
        })
        .collect()
}

async fn mark_scope_processor_finished(scope_key: &str) {
    if let Ok(mut active) = active_scope_processors().lock() {
        active.remove(scope_key);
    }
}

async fn process_scope_queue(scope: ShepherdScope, scope_key: String) {
    let store = match ShepherdChatStore::open().await {
        Ok(store) => store,
        Err(error) => {
            tracing::error!(%error, %scope_key, "failed to open shepherd chat store for queue processor");
            mark_scope_processor_finished(&scope_key).await;
            return;
        }
    };

    let (project_id, runtime_name) = scope_storage_ids(&scope);

    loop {
        let next = match store
            .claim_next_turn(project_id, runtime_name.as_deref())
            .await
        {
            Ok(next) => next,
            Err(error) => {
                tracing::error!(%error, %scope_key, "failed to claim shepherd queue item");
                break;
            }
        };

        let Some(job) = next else {
            break;
        };

        let focus = job
            .focus_json
            .as_deref()
            .and_then(|json| serde_json::from_str::<ShepherdTaskFocus>(json).ok());
        let user_chunks = parse_chunks_from_json(&job.chunks_json);

        let result = run_shepherd_turn_for_scope(
            &format!("queued-turn-{}", job.id),
            &scope,
            user_chunks,
            focus,
            true,
        )
        .await;

        match result {
            Ok(chunks) => {
                if let ShepherdScope::Thread { thread_id, .. } = &scope {
                    let summary = result_summary_from_chunks(&chunks);
                    if let Ok(store) = ShepherdThreadStore::open().await {
                        let _ = store
                            .update_thread(
                                thread_id,
                                None,
                                None,
                                Some(&summary),
                                Some("running"),
                                None,
                                None,
                            )
                            .await;
                    }
                }
                if let Err(error) = store.complete_turn(job.id).await {
                    tracing::error!(%error, queue_id = job.id, %scope_key, "failed to mark shepherd queue item complete");
                }
            }
            Err(error) => {
                if let ShepherdScope::Thread { thread_id, .. } = &scope {
                    if let Ok(store) = ShepherdThreadStore::open().await {
                        let _ = store
                            .update_thread(
                                thread_id,
                                None,
                                None,
                                Some(&error),
                                Some("failed"),
                                None,
                                None,
                            )
                            .await;
                    }
                }
                tracing::error!(%error, queue_id = job.id, %scope_key, "queued shepherd turn failed");
                if let Err(mark_error) = store.fail_turn(job.id, &error).await {
                    tracing::error!(%mark_error, queue_id = job.id, %scope_key, "failed to mark shepherd queue item failed");
                }
            }
        }
    }

    mark_scope_processor_finished(&scope_key).await;
}

fn kick_scope_queue_processor(scope: ShepherdScope) -> Result<(), String> {
    let Some(scope_key) = scope_queue_key(&scope) else {
        return Err("this scope cannot own durable queued turns".to_string());
    };

    {
        let mut active = active_scope_processors()
            .lock()
            .map_err(|_| "failed to lock shepherd queue processor set".to_string())?;
        if active.contains(&scope_key) {
            return Ok(());
        }
        active.insert(scope_key.clone());
    }

    tokio::spawn(async move {
        process_scope_queue(scope, scope_key).await;
    });

    Ok(())
}

#[tracing::instrument(skip(user_chunks))]
async fn run_shepherd_turn_for_scope(
    runtime_id: &str,
    scope: &ShepherdScope,
    user_chunks: Vec<ShepherdMessageChunk>,
    focus: Option<ShepherdTaskFocus>,
    persist_input_message: bool,
) -> Result<Vec<ShepherdMessageChunk>, String> {
    let focus = focus.or_else(|| match scope {
        ShepherdScope::Project { focus, .. } | ShepherdScope::Thread { focus, .. } => focus.clone(),
        _ => None,
    });
    let user_chunks_json = chunks_to_json(&user_chunks)?;
    let user_images_png = decode_png_images(&user_chunks)?;
    let user_turn_text = build_user_turn_text(&user_chunks);
    let history = load_scope_messages(scope, RUNTIME_HISTORY_LIMIT).await?;
    let cwd = resolve_scope_workspace(scope).await?;
    let scope_project_id = resolve_scope_project_id(scope).await;
    tracing::info!("starting shepherd turn");

    let runtime = if let Some(state) = load_scope_state(scope).await? {
        create_runtime_from_state(
            None,
            runtime_id,
            scope,
            focus.as_ref(),
            scope_project_id,
            &cwd,
            state,
        )
        .await?
    } else {
        create_runtime_from_history(
            None,
            runtime_id,
            scope,
            focus.as_ref(),
            scope_project_id,
            &cwd,
            &history,
        )
        .await?
    };

    if persist_input_message {
        save_message(scope, "user", &user_chunks_json).await?;
    }

    let cancel = CancellationToken::new();
    let mut turn_items = Vec::new();
    turn_items.push(InputItem::Text {
        text: user_turn_text,
    });
    let mut image_blobs = HashMap::new();
    for (idx, bytes) in user_images_png.into_iter().enumerate() {
        let id = format!("image-{}", idx + 1);
        turn_items.push(InputItem::ImageRef { id: id.clone() });
        image_blobs.insert(id, bytes);
    }

    let mut runtime = runtime;
    let turn = runtime
        .stream_turn(
            TurnInput {
                items: turn_items,
                image_blobs,
                mode: None,
            },
            &SilentLashSink,
            cancel,
        )
        .await
        .map_err(|e| format!("failed to run shepherd turn: {}", e))?;

    let final_text = ShepherdLashSink::sanitize_assistant_text(&turn.assistant_output.safe_text);
    let mut assistant_chunks = Vec::new();
    if !final_text.trim().is_empty() {
        assistant_chunks.push(ShepherdMessageChunk::Text {
            content: final_text,
        });
    }
    assistant_chunks.extend(tool_chunks_from_records(&turn.tool_calls));

    if assistant_chunks.is_empty() {
        tracing::error!("shepherd turn completed without user-visible output");
        return Err("Shepherd returned no user-visible output for this turn.".to_string());
    }

    let assistant_chunks_json = chunks_to_json(&assistant_chunks)?;
    save_message(scope, "assistant", &assistant_chunks_json).await?;
    let state = runtime.export_state();
    save_scope_state(scope, &state).await?;
    if let ShepherdScope::Thread { thread_id, .. } = scope {
        if let Ok(store) = ShepherdThreadStore::open().await {
            let summary = result_summary_from_chunks(&assistant_chunks);
            let _ = store
                .update_thread(
                    thread_id,
                    None,
                    None,
                    Some(&summary),
                    Some("running"),
                    None,
                    None,
                )
                .await;
        }
    }
    tracing::info!(
        chunk_count = assistant_chunks.len(),
        "shepherd turn completed"
    );
    Ok(assistant_chunks)
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

fn project_survey_objective(route_name: &str) -> String {
    format!(
        "Survey route `{route_name}` from an isolated checkout, refresh the canvas and retained context when stale, and summarize the current project picture."
    )
}

fn project_survey_prompt(project_name: &str, route_name: &str) -> String {
    format!(
        "Survey the `{project_name}` codebase on route `{route_name}` from your isolated checkout. Read the workspace, refresh the canvas HTML artifact if it is stale or incomplete, refresh retained context if it is stale, and summarize the architecture, pressure points, and next useful threads. Use `update_plan` so the thread card stays legible."
    )
}

fn thread_scope(thread: &ShepherdThread) -> ShepherdScope {
    ShepherdScope::Thread {
        project_id: thread.project_id,
        route_id: thread.route_id,
        thread_id: thread.id.clone(),
        title: thread.title.clone(),
        workspace_path: thread.workspace_path.clone(),
        focus: None,
    }
}

async fn thread_has_active_turn(thread: &ShepherdThread) -> Result<bool, String> {
    let items = load_scope_queue(&thread_scope(thread)).await?;
    Ok(items
        .iter()
        .any(|item| matches!(item.status.as_str(), "pending" | "working")))
}

#[tracing::instrument(fields(project_id, route_id))]
pub async fn launch_project_survey_thread(
    project_id: i64,
    route_id: Option<i64>,
) -> Result<ShepherdThread, String> {
    let project_store = ProjectStore::open()
        .await
        .map_err(|e| format!("failed to open project store: {}", e))?;
    let project = project_store
        .get_project(project_id)
        .await
        .map_err(|e| format!("failed to load project {}: {}", project_id, e))?;
    let route = resolve_target_route(project_id, route_id).await?;
    let thread_store = ShepherdThreadStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd thread store: {}", e))?;
    let objective = project_survey_objective(&route.name);
    let queued_summary = format!(
        "Surveying route `{}` and refreshing the shared project picture.",
        route.name
    );

    let mut thread = match thread_store
        .find_route_thread_by_title(project_id, route.id, PROJECT_SURVEY_THREAD_TITLE)
        .await
        .map_err(|e| format!("failed to look up project survey thread: {}", e))?
    {
        Some(existing) => {
            let missing_workspace = existing
                .workspace_path
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
                || existing
                    .checkout_name
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty());
            let (workspace_path, checkout_name) = if missing_workspace {
                let (workspace_path, checkout_name) =
                    prepare_thread_workspace(project_id, route.id, PROJECT_SURVEY_THREAD_TITLE)
                        .await?;
                (Some(workspace_path), Some(checkout_name))
            } else {
                (None, None)
            };
            thread_store
                .update_thread(
                    &existing.id,
                    Some(PROJECT_SURVEY_THREAD_TITLE),
                    Some(&objective),
                    None,
                    Some("running"),
                    workspace_path.as_deref().map(Some),
                    checkout_name.as_deref().map(Some),
                )
                .await
                .map_err(|e| format!("failed to refresh project survey thread: {}", e))?;
            thread_store
                .get_thread(&existing.id)
                .await
                .map_err(|e| format!("failed to reload project survey thread: {}", e))?
        }
        None => {
            let (workspace_path, checkout_name) =
                prepare_thread_workspace(project_id, route.id, PROJECT_SURVEY_THREAD_TITLE).await?;
            thread_store
                .create_thread(
                    project_id,
                    route.id,
                    PROJECT_SURVEY_THREAD_TITLE,
                    &objective,
                    &queued_summary,
                    Some(&workspace_path),
                    Some(&checkout_name),
                )
                .await
                .map_err(|e| format!("failed to create project survey thread: {}", e))?
        }
    };

    if thread_has_active_turn(&thread).await? {
        tracing::info!(thread_id = %thread.id, "project survey thread already queued");
        return Ok(thread);
    }

    thread_store
        .update_thread(
            &thread.id,
            None,
            Some(&objective),
            Some(&queued_summary),
            Some("running"),
            None,
            None,
        )
        .await
        .map_err(|e| format!("failed to update project survey thread: {}", e))?;
    thread = thread_store
        .get_thread(&thread.id)
        .await
        .map_err(|e| format!("failed to reload project survey thread: {}", e))?;

    enqueue_shepherd_message_for_scope(
        thread_scope(&thread),
        Some(project_survey_prompt(&project.name, &route.name)),
        None,
        None,
    )
    .await?;

    Ok(thread)
}

pub async fn enqueue_shepherd_message_for_scope(
    scope: ShepherdScope,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
    focus: Option<ShepherdTaskFocus>,
) -> Result<EnqueueShepherdMessageResponse, String> {
    let user_chunks = build_user_chunks(content, chunks)?;
    let chunks_json = chunks_to_json(&user_chunks)?;
    let focus_json = focus
        .as_ref()
        .map(|value| {
            serde_json::to_string(value).map_err(|e| format!("failed to serialize focus: {}", e))
        })
        .transpose()?;
    let store = ShepherdChatStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd chat store: {}", e))?;
    let (project_id, runtime_name) = scope_storage_ids(&scope);
    store
        .enqueue_turn(
            project_id,
            runtime_name.as_deref(),
            &chunks_json,
            focus_json.as_deref(),
        )
        .await
        .map_err(|e| format!("failed to enqueue shepherd message: {}", e))?;
    kick_scope_queue_processor(scope.clone())?;
    let queue_items = load_scope_queue(&scope).await?;

    Ok(EnqueueShepherdMessageResponse {
        queued: true,
        queue_depth: queue_items
            .iter()
            .filter(|item| item.status == "pending")
            .count(),
        thread_id: match scope {
            ShepherdScope::Thread { thread_id, .. } => Some(thread_id),
            _ => None,
        },
    })
}

pub async fn enqueue_project_message(
    project_id: i64,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
) -> Result<EnqueueShepherdMessageResponse, String> {
    let user_chunks = build_user_chunks(content, chunks)?;
    let route = resolve_target_route(project_id, None).await?;
    let scope = ShepherdScope::Project {
        project_id,
        route_id: route.id,
        workspace_path: None,
        focus: None,
    };
    enqueue_shepherd_message_for_scope(scope, None, Some(user_chunks), None).await
}

pub async fn get_route_threads(
    project_id: i64,
    route_id: i64,
) -> Result<Vec<ShepherdThread>, String> {
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd thread store: {}", e))?;
    store
        .list_route_threads(project_id, route_id)
        .await
        .map_err(|e| format!("failed to load route threads: {}", e))
}

pub async fn get_route_conversation(
    project_id: i64,
    route_id: i64,
) -> Result<Vec<ShepherdChatMessage>, String> {
    load_scope_messages(
        &ShepherdScope::Project {
            project_id,
            route_id,
            workspace_path: None,
            focus: None,
        },
        100,
    )
    .await
}

pub async fn get_route_queue(project_id: i64, route_id: i64) -> Result<ShepherdQueueState, String> {
    let items = load_scope_queue(&ShepherdScope::Project {
        project_id,
        route_id,
        workspace_path: None,
        focus: None,
    })
    .await?;
    let has_active_turn = items.iter().any(|item| item.status == "working");
    Ok(ShepherdQueueState {
        items,
        has_active_turn,
    })
}

pub async fn get_thread_conversation(
    project_id: i64,
    route_id: i64,
    thread_id: &str,
    title: &str,
    limit: usize,
) -> Result<Vec<ShepherdChatMessage>, String> {
    load_scope_messages(
        &ShepherdScope::Thread {
            project_id,
            route_id,
            thread_id: thread_id.to_string(),
            title: title.to_string(),
            workspace_path: None,
            focus: None,
        },
        limit,
    )
    .await
}

pub async fn get_thread_queue(
    project_id: i64,
    route_id: i64,
    thread_id: &str,
    title: &str,
) -> Result<ShepherdQueueState, String> {
    let items = load_scope_queue(&ShepherdScope::Thread {
        project_id,
        route_id,
        thread_id: thread_id.to_string(),
        title: title.to_string(),
        workspace_path: None,
        focus: None,
    })
    .await?;
    let has_active_turn = items.iter().any(|item| item.status == "working");
    Ok(ShepherdQueueState {
        items,
        has_active_turn,
    })
}

pub async fn get_shepherd_queue(scope: ShepherdScope) -> Result<ShepherdQueueState, String> {
    let items = load_scope_queue(&scope).await?;
    let has_active_turn = items.iter().any(|item| item.status == "working");
    Ok(ShepherdQueueState {
        items,
        has_active_turn,
    })
}

/// Get Shepherd chat history for the requested scope.
pub async fn get_shepherd_history(
    scope: ShepherdScope,
    limit: usize,
) -> Result<Vec<ShepherdChatMessage>, String> {
    let messages = load_scope_messages(&scope, limit).await?;

    Ok(match scope {
        ShepherdScope::Project { .. } | ShepherdScope::Thread { .. } => messages,
        _ => messages.into_iter().take(limit).collect(),
    })
}
