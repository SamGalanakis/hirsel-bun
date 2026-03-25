use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use lash::{
    default_context_strategy, default_execution_mode, AgentStateEnvelope, EventSink, ExecutionMode,
    HostProfile, InputItem, LashRuntime, PluginHost, RuntimeHostConfig, RuntimeServices,
    SessionPolicy, ToolProvider, TurnInput,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::history::{
    build_runtime_messages, build_user_chunks, chunks_to_json, decode_png_images,
    load_scope_messages, load_scope_queue, parse_chunks_from_json, save_message,
    RUNTIME_HISTORY_LIMIT,
};
use super::router::{focus_effort, focused_or_latest_effort, route_message_to_effort};
use super::runtime::{
    build_user_turn_text, looks_like_runtime_traceback, resolve_scope_project_id,
    resolve_scope_workspace, shepherd_prompt_overrides, ShepherdLashSink,
};
use super::tools::ShepherdToolProvider;
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
use crate::core::credentials::CredentialStore;
use crate::core::delta::DeltaState;
use crate::core::{
    ensure_sync_project_task, llm_provider, ProjectStore, Route, RouteStore, ShepherdChatMessage,
    ShepherdChatStore, ShepherdEffort, ShepherdQueuedTurn, WorkItem,
};
use crate::lash_tools::embedded_shepherd_plugin_factories;

pub(super) struct SilentLashSink;

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
pub struct StartProjectSyncResponse {
    pub route_id: i64,
    pub item: WorkItem,
    pub started: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueShepherdMessageResponse {
    pub queued: bool,
    pub queue_depth: usize,
    #[serde(default)]
    pub effort_id: Option<String>,
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
    let plugin_factories = embedded_shepherd_plugin_factories(
        "hirsel_shepherd_tools",
        Arc::clone(&tools),
        load_tavily_api_key().await,
    );
    let plugin_host = PluginHost::new(plugin_factories).with_dynamic_tools();
    let root_plugins = plugin_host
        .build_session(agent_id, execution_mode, None)
        .map_err(|e| format!("failed to build shepherd tool session: {}", e))?;
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
    let (hirsel_config, _) =
        crate::core::config::Config::load().map_err(|e| format!("failed to load config: {}", e))?;
    let provider = llm_provider::resolve_provider(&hirsel_config).await?;
    let (model, model_variant) = llm_provider::resolve_model(&hirsel_config, &provider);
    let execution_mode = default_execution_mode();
    let context_strategy = default_context_strategy();
    let session_policy = SessionPolicy {
        model: model.clone(),
        provider,
        max_context_tokens: Some(crate::core::config::get_context_window(&model) as usize),
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
        ShepherdScope::Effort {
            project_id,
            effort_id,
            ..
        } => Some(format!("effort:{}:{}", project_id, effort_id)),
        ShepherdScope::Branch { .. } => None,
    }
}

fn scope_storage_ids(scope: &ShepherdScope) -> (Option<i64>, Option<String>) {
    match scope {
        ShepherdScope::General => (None, None),
        ShepherdScope::Project { project_id, .. } => (
            Some(*project_id),
            Some(ShepherdChatStore::PROJECT_CHAT_RUN_NAME.to_string()),
        ),
        ShepherdScope::Effort {
            project_id,
            effort_id,
            ..
        } => (
            Some(*project_id),
            Some(ShepherdChatStore::effort_runtime_name(effort_id)),
        ),
        ShepherdScope::Branch { .. } => (None, None),
    }
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
        .map_err(|e| format!("failed to create shepherd branch runtime: {}", e))
}

fn truncate_internal_note(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

#[tracing::instrument(skip(user_images_png, cancel, parent_state))]
async fn run_private_branch_turn(
    app: Option<&tauri::AppHandle>,
    parent_session_id: &str,
    parent_scope: &ShepherdScope,
    parent_focus: Option<ShepherdTaskFocus>,
    user_turn_text: &str,
    user_images_png: &[Vec<u8>],
    cancel: CancellationToken,
    parent_state: AgentStateEnvelope,
) -> Result<Option<String>, String> {
    let (project_id, route_id, workspace_path) = match parent_scope {
        ShepherdScope::Project {
            project_id,
            route_id,
            workspace_path,
            ..
        }
        | ShepherdScope::Effort {
            project_id,
            route_id,
            workspace_path,
            ..
        } => (*project_id, *route_id, workspace_path.clone()),
        _ => return Ok(None),
    };

    let branch_id = uuid::Uuid::new_v4().to_string();
    let branch_scope = ShepherdScope::Branch {
        project_id,
        route_id,
        branch_id: branch_id.clone(),
        parent_session_id: parent_session_id.to_string(),
        goal: "Think through the latest user request, inspect Hirsel state, use tools if needed, and return a private conclusion for the channel.".to_string(),
        workspace_path,
        focus: parent_focus,
    };
    let cwd = resolve_scope_workspace(&branch_scope).await?;
    let scope_project_id = resolve_scope_project_id(&branch_scope).await;
    tracing::info!("starting private shepherd branch turn");

    let mut branch_runtime = create_runtime_from_state(
        app,
        &branch_id,
        &branch_scope,
        match &branch_scope {
            ShepherdScope::Branch { focus, .. } => focus.as_ref(),
            _ => None,
        },
        scope_project_id,
        &cwd,
        parent_state,
    )
    .await?;

    let mut turn_items = vec![InputItem::Text {
        text: user_turn_text.to_string(),
    }];
    let mut image_blobs = HashMap::new();
    for (idx, bytes) in user_images_png.iter().enumerate() {
        let id = format!("branch-image-{}", idx + 1);
        turn_items.push(InputItem::ImageRef { id: id.clone() });
        image_blobs.insert(id, bytes.clone());
    }

    let turn = branch_runtime
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
        .map_err(|e| format!("failed to run private shepherd branch: {}", e))?;

    let conclusion =
        super::runtime::ShepherdLashSink::sanitize_assistant_text(&turn.assistant_output.safe_text);
    if conclusion.trim().is_empty() || looks_like_runtime_traceback(&conclusion) {
        tracing::warn!("private shepherd branch produced no usable conclusion");
        return Ok(None);
    }

    tracing::info!("private shepherd branch completed");
    Ok(Some(truncate_internal_note(&conclusion, 4000)))
}

#[tracing::instrument(skip(prompt), fields(project_id, route_id = route.id, item_id = %item_id))]
async fn run_project_sync_branch(
    project_id: i64,
    route: Route,
    item_id: String,
    item_title: String,
    prompt: String,
) -> Result<Option<String>, String> {
    let scope = ShepherdScope::Project {
        project_id,
        route_id: route.id,
        workspace_path: None,
        focus: Some(ShepherdTaskFocus {
            task_id: item_id.clone(),
            task_name: item_title.clone(),
        }),
    };
    let cwd = resolve_scope_workspace(&scope).await?;
    let history = load_scope_messages(&scope, RUNTIME_HISTORY_LIMIT).await?;
    let runtime = create_runtime_from_history(
        None,
        &format!("project-sync-{}", uuid::Uuid::new_v4()),
        &scope,
        match &scope {
            ShepherdScope::Project { focus, .. } => focus.as_ref(),
            _ => None,
        },
        Some(project_id),
        &cwd,
        &history,
    )
    .await?;
    let parent_state = runtime.export_state();
    drop(runtime);

    run_private_branch_turn(
        None,
        &format!("project-sync-parent-{}", uuid::Uuid::new_v4()),
        &scope,
        match &scope {
            ShepherdScope::Project { focus, .. } => focus.clone(),
            _ => None,
        },
        &format!(
            "{}\n\nDo not call the `sync_project` tool from this run. This run is already the project sync.",
            prompt
        ),
        &[],
        CancellationToken::new(),
        parent_state,
    )
    .await
}

async fn finish_project_sync(
    project_id: i64,
    route_id: i64,
    item_id: &str,
    status: &'static str,
    summary: &str,
    details: Option<&str>,
) {
    let state = DeltaState::with_route(project_id, route_id);
    let result = match status {
        "done" => state.complete_work_item(item_id, "channel:main").await,
        "failed" => state.fail_work_item(item_id, "channel:main").await,
        _ => unreachable!("unexpected sync terminal status"),
    };

    if let Err(error) = result {
        tracing::error!(%error, project_id, route_id, item_id, "failed to update sync task terminal status");
        return;
    }

    if let Err(error) = state
        .record_item_event(item_id, "channel", "main", "sync_status", summary, details)
        .await
    {
        tracing::warn!(%error, project_id, route_id, item_id, "failed to record sync task event");
    }
}

#[tracing::instrument(fields(project_id, route_id = route.id, item_id = %item.id))]
fn spawn_project_sync_task(
    project_id: i64,
    route: Route,
    item: WorkItem,
    prompt: String,
    effort_id: Option<String>,
) {
    tokio::spawn(async move {
        tracing::info!("starting detached project sync task");
        match run_project_sync_branch(
            project_id,
            route.clone(),
            item.id.clone(),
            item.title.clone(),
            prompt,
        )
        .await
        {
            Ok(conclusion) => {
                if let Some(effort_id) = effort_id.as_deref() {
                    if let Ok(store) = ShepherdChatStore::open().await {
                        let summary = conclusion
                            .as_deref()
                            .map(|text| truncate_internal_note(text, 240))
                            .map(|text| text.to_string())
                            .unwrap_or_else(|| "Project sync completed.".to_string());
                        let _ = store
                            .update_effort(effort_id, Some(&summary), Some("done"))
                            .await;
                    }
                    let completion_message = conclusion
                        .as_deref()
                        .filter(|text| !text.trim().is_empty())
                        .map(|text| format!("Project sync completed.\n\n{}", text.trim()))
                        .unwrap_or_else(|| "Project sync completed.".to_string());
                    if let Err(error) = save_effort_assistant_note(
                        project_id,
                        route.id,
                        effort_id,
                        &item.id,
                        "Project sync",
                        &completion_message,
                    )
                    .await
                    {
                        tracing::warn!(%error, effort_id, "failed to save sync completion note");
                    }
                }
                let details = conclusion.as_deref();
                finish_project_sync(
                    project_id,
                    route.id,
                    &item.id,
                    "done",
                    "Project sync completed.",
                    details,
                )
                .await;
                tracing::info!("detached project sync task completed");
            }
            Err(error) => {
                if let Some(effort_id) = effort_id.as_deref() {
                    if let Ok(store) = ShepherdChatStore::open().await {
                        let _ = store
                            .update_effort(effort_id, Some(&error), Some("failed"))
                            .await;
                    }
                    let failure_message = format!("Project sync failed.\n\n{}", error.trim());
                    if let Err(note_error) = save_effort_assistant_note(
                        project_id,
                        route.id,
                        effort_id,
                        &item.id,
                        "Project sync",
                        &failure_message,
                    )
                    .await
                    {
                        tracing::warn!(%note_error, effort_id, "failed to save sync failure note");
                    }
                }
                let details = error.clone();
                finish_project_sync(
                    project_id,
                    route.id,
                    &item.id,
                    "failed",
                    "Project sync failed.",
                    Some(&details),
                )
                .await;
                tracing::error!(%error, "detached project sync task failed");
            }
        }
    });
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
            true,
        )
        .await;

        match result {
            Ok(chunks) => {
                if let ShepherdScope::Effort { effort_id, .. } = &scope {
                    let summary = result_summary_from_chunks(&chunks);
                    if let Ok(store) = ShepherdChatStore::open().await {
                        let _ = store
                            .update_effort(effort_id, Some(&summary), Some("active"))
                            .await;
                    }
                }
                if let Err(error) = store.complete_turn(job.id).await {
                    tracing::error!(%error, queue_id = job.id, %scope_key, "failed to mark shepherd queue item complete");
                }
            }
            Err(error) => {
                if let ShepherdScope::Effort { effort_id, .. } = &scope {
                    if let Ok(store) = ShepherdChatStore::open().await {
                        let _ = store
                            .update_effort(effort_id, Some(&error), Some("failed"))
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
        return Err("branch scope cannot own durable queued turns".to_string());
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
    use_private_branch: bool,
) -> Result<Vec<ShepherdMessageChunk>, String> {
    let focus = focus.or_else(|| match scope {
        ShepherdScope::Project { focus, .. }
        | ShepherdScope::Effort { focus, .. }
        | ShepherdScope::Branch { focus, .. } => focus.clone(),
        _ => None,
    });
    let user_chunks_json = chunks_to_json(&user_chunks)?;
    let user_images_png = decode_png_images(&user_chunks)?;
    let user_turn_text = build_user_turn_text(&user_chunks);
    let history = load_scope_messages(scope, RUNTIME_HISTORY_LIMIT).await?;
    let cwd = resolve_scope_workspace(scope).await?;
    let scope_project_id = resolve_scope_project_id(scope).await;
    tracing::info!("starting shepherd turn");

    let runtime = create_runtime_from_history(
        None,
        runtime_id,
        scope,
        focus.as_ref(),
        scope_project_id,
        &cwd,
        &history,
    )
    .await?;

    if persist_input_message {
        save_message(scope, "user", &user_chunks_json).await?;
    }

    let cancel = CancellationToken::new();
    let branch_conclusion = if use_private_branch {
        let parent_state = runtime.export_state();
        run_private_branch_turn(
            None,
            runtime_id,
            scope,
            focus.clone(),
            &user_turn_text,
            &user_images_png,
            cancel.clone(),
            parent_state,
        )
        .await?
    } else {
        None
    };

    let mut turn_items = Vec::new();
    if let Some(branch_conclusion) = branch_conclusion.as_ref() {
        turn_items.push(InputItem::Text {
            text: format!(
                "Private branch conclusion for this turn. Use it as internal reasoning context only. Do not mention branching, hidden analysis, or this note to the user.\n\n{}",
                branch_conclusion
            ),
        });
    }
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
    if let ShepherdScope::Effort { effort_id, .. } = scope {
        if let Ok(store) = ShepherdChatStore::open().await {
            let summary = result_summary_from_chunks(&assistant_chunks);
            let _ = store
                .update_effort(effort_id, Some(&summary), Some("active"))
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

async fn save_effort_assistant_note(
    project_id: i64,
    route_id: i64,
    effort_id: &str,
    work_item_id: &str,
    title: &str,
    message: &str,
) -> Result<(), String> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let scope = ShepherdScope::Effort {
        project_id,
        route_id,
        effort_id: effort_id.to_string(),
        title: title.to_string(),
        workspace_path: None,
        focus: Some(ShepherdTaskFocus {
            task_id: work_item_id.to_string(),
            task_name: title.to_string(),
        }),
    };
    let chunks_json = chunks_to_json(&[ShepherdMessageChunk::Text {
        content: trimmed.to_string(),
    }])?;
    save_message(&scope, "assistant", &chunks_json).await?;
    Ok(())
}

#[tracing::instrument(fields(project_id, route_id))]
pub async fn start_project_sync(
    project_id: i64,
    route_id: Option<i64>,
    refresh: bool,
) -> Result<StartProjectSyncResponse, String> {
    let project_store = ProjectStore::open()
        .await
        .map_err(|e| format!("failed to open project store: {}", e))?;
    let project = project_store
        .get_project(project_id)
        .await
        .map_err(|e| format!("failed to load project {}: {}", project_id, e))?;
    let route = resolve_target_route(project_id, route_id).await?;
    let result = ensure_sync_project_task(project_id, &route, &project.name, true, refresh)
        .await
        .map_err(|e| format!("failed to prepare project sync task: {}", e))?;
    let state = DeltaState::with_route(project_id, route.id);

    if result.item.status == "working" {
        tracing::info!("project sync already running");
        return Ok(StartProjectSyncResponse {
            route_id: route.id,
            item: result.item,
            started: false,
        });
    }

    let item = state
        .start_work_item(&result.item.id)
        .await
        .map(WorkItem::from)
        .map_err(|e| format!("failed to start sync task {}: {}", result.item.id, e))?;

    if let Err(error) = state
        .record_item_event(
            &item.id,
            "channel",
            "main",
            "sync_started",
            "Project sync started.",
            Some("The backend is surveying the project and updating Hirsel in the background."),
        )
        .await
    {
        tracing::warn!(%error, item_id = %item.id, "failed to record sync start event");
    }

    let mut sync_effort_id = None;

    // Create a focused effort so sync appears in the normal effort UI
    if let Ok(chat_store) = ShepherdChatStore::open().await {
        match chat_store
            .create_effort(
                project_id,
                route.id,
                &item.id,
                "Project sync",
                "Surveying the codebase and building the project picture.",
                true, // focused
            )
            .await
        {
            Ok(effort) => {
                if let Err(error) = save_effort_assistant_note(
                    project_id,
                    route.id,
                    &effort.id,
                    &item.id,
                    &effort.title,
                    "Project sync started.\n\nI’m surveying the workspace and assembling the current project picture. Progress and conclusions will appear here.",
                )
                .await
                {
                    tracing::warn!(%error, effort_id = %effort.id, "failed to save sync start note");
                }
                sync_effort_id = Some(effort.id);
            }
            Err(e) => {
                tracing::warn!(%e, "failed to create sync effort");
            }
        }
    }

    spawn_project_sync_task(
        project_id,
        route,
        item.clone(),
        result.prompt,
        sync_effort_id,
    );

    Ok(StartProjectSyncResponse {
        route_id: result.route_id,
        item,
        started: true,
    })
}

pub async fn enqueue_shepherd_message_for_scope(
    scope: ShepherdScope,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
    focus: Option<ShepherdTaskFocus>,
) -> Result<EnqueueShepherdMessageResponse, String> {
    if matches!(scope, ShepherdScope::Branch { .. }) {
        return Err("Branch scopes cannot accept durable user-facing messages".to_string());
    }

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
        effort_id: match scope {
            ShepherdScope::Effort { effort_id, .. } => Some(effort_id),
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
    let message_text = build_user_turn_text(&user_chunks);
    let route = resolve_target_route(project_id, None).await?;
    let effort = route_message_to_effort(project_id, route.id, &route.name, &message_text).await?;
    let scope = ShepherdScope::Effort {
        project_id,
        route_id: route.id,
        effort_id: effort.id.clone(),
        title: effort.title.clone(),
        workspace_path: None,
        focus: Some(ShepherdTaskFocus {
            task_id: effort.work_item_id.clone(),
            task_name: effort.title.clone(),
        }),
    };
    let response = enqueue_shepherd_message_for_scope(scope, None, Some(user_chunks), None).await?;
    if let Ok(store) = ShepherdChatStore::open().await {
        let _ = store
            .update_effort(&effort.id, Some(&effort.summary), Some("active"))
            .await;
    }
    Ok(response)
}

pub async fn get_project_efforts(
    project_id: i64,
    route_id: i64,
) -> Result<Vec<ShepherdEffort>, String> {
    let store = ShepherdChatStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd chat store: {}", e))?;
    store
        .list_project_efforts(project_id, route_id)
        .await
        .map_err(|e| format!("failed to load project efforts: {}", e))
}

pub async fn get_focused_project_effort(
    project_id: i64,
    route_id: i64,
) -> Result<Option<ShepherdEffort>, String> {
    focused_or_latest_effort(project_id, route_id).await
}

pub async fn focus_project_effort(
    project_id: i64,
    route_id: i64,
    effort_id: String,
) -> Result<ShepherdEffort, String> {
    focus_effort(project_id, route_id, &effort_id).await
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
        ShepherdScope::Project { .. } | ShepherdScope::Effort { .. } => messages,
        _ => messages.into_iter().take(limit).collect(),
    })
}
