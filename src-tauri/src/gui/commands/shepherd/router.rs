use std::sync::{Arc, Mutex as StdMutex};

use lash::plugin::{PluginFactory, StaticPluginFactory};
use lash::{
    default_context_strategy, default_execution_mode, AgentStateEnvelope,
    BuiltinToolResultProjectionPluginFactory, ExecutionMode, HostProfile, InputItem, LashRuntime,
    PluginHost, PluginSpec, RuntimeHostConfig, RuntimeServices, SessionPolicy, ToolDefinition,
    ToolParam, ToolProvider, ToolResult, TurnInput,
};
use serde_json::{json, Value};

use super::commands::SilentLashSink;
use crate::core::{llm_provider, CapabilityProfile, DeltaState, ShepherdChatStore, ShepherdEffort};

macro_rules! tool_definition {
    ($($field:tt)*) => {
        ToolDefinition {
            $($field)*
            input_schema_override: None,
            output_schema_override: None,
        }
    };
}

#[derive(Debug, Clone)]
pub(super) enum RouterDecision {
    RouteTo { effort_id: String },
    Create { title: String, summary: String },
}

#[derive(Clone)]
struct RouterDecisionState {
    inner: Arc<StdMutex<Option<RouterDecision>>>,
}

impl RouterDecisionState {
    fn new() -> Self {
        Self {
            inner: Arc::new(StdMutex::new(None)),
        }
    }

    fn set(&self, decision: RouterDecision) -> Result<(), String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "failed to lock router decision".to_string())?;
        *guard = Some(decision);
        Ok(())
    }

    fn take(&self) -> Result<Option<RouterDecision>, String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "failed to lock router decision".to_string())?;
        Ok(guard.take())
    }
}

#[derive(Clone)]
struct RouterToolProvider {
    efforts: Vec<ShepherdEffort>,
    decision: RouterDecisionState,
}

impl RouterToolProvider {
    fn new(efforts: Vec<ShepherdEffort>, decision: RouterDecisionState) -> Self {
        Self { efforts, decision }
    }

    fn trimmed<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
        args.get(key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

#[async_trait::async_trait]
impl ToolProvider for RouterToolProvider {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![
            tool_definition! {
                name: "list_active_efforts".to_string(),
                description: "List the currently active efforts for this route, including which one is focused.".to_string(),
                params: vec![],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "route_to_effort".to_string(),
                description: "Route this user message into an existing effort. Use this for follow-ups or related work.".to_string(),
                params: vec![
                    ToolParam::typed("effort_id", "str"),
                    ToolParam::optional("reason", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "create_effort".to_string(),
                description: "Create a new effort when the message is materially separate from active efforts. Title should be short and concrete.".to_string(),
                params: vec![
                    ToolParam::typed("title", "str"),
                    ToolParam::typed("summary", "str"),
                    ToolParam::optional("reason", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
        ]
    }

    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        match name {
            "list_active_efforts" => ToolResult::ok(json!({
                "efforts": self.efforts.iter().map(|effort| {
                    json!({
                        "id": effort.id,
                        "title": effort.title,
                        "summary": effort.summary,
                        "status": effort.status,
                        "focused": effort.focused,
                        "work_item_id": effort.work_item_id,
                        "last_activity_at": effort.last_activity_at,
                    })
                }).collect::<Vec<_>>()
            })),
            "route_to_effort" => {
                let Some(effort_id) = Self::trimmed(args, "effort_id") else {
                    return ToolResult::err_fmt("Missing required parameter: effort_id");
                };
                if !self.efforts.iter().any(|effort| effort.id == effort_id) {
                    return ToolResult::err(json!({
                        "error": format!("Unknown effort '{}'", effort_id),
                    }));
                }
                if let Err(error) = self.decision.set(RouterDecision::RouteTo {
                    effort_id: effort_id.to_string(),
                }) {
                    return ToolResult::err(json!({ "error": error }));
                }
                ToolResult::ok(json!({ "ok": true, "effort_id": effort_id }))
            }
            "create_effort" => {
                let Some(title) = Self::trimmed(args, "title") else {
                    return ToolResult::err_fmt("Missing required parameter: title");
                };
                let Some(summary) = Self::trimmed(args, "summary") else {
                    return ToolResult::err_fmt("Missing required parameter: summary");
                };
                if let Err(error) = self.decision.set(RouterDecision::Create {
                    title: title.to_string(),
                    summary: summary.to_string(),
                }) {
                    return ToolResult::err(json!({ "error": error }));
                }
                ToolResult::ok(json!({
                    "ok": true,
                    "title": title,
                    "summary": summary,
                }))
            }
            _ => ToolResult::err(json!({ "error": format!("Unknown tool: {}", name) })),
        }
    }
}

fn build_router_guidance(route_name: &str) -> String {
    format!(
        "## Hirsel Intake Router\n\n\
         You are routing one new user message for route `{}`.\n\n\
         Your job is to decide whether the message belongs to an existing effort or needs a new effort.\n\n\
         Rules:\n\
         - Keep related follow-ups on the same effort.\n\
         - Create a new effort only when the user intent is materially separate from active efforts.\n\
         - Always call `list_active_efforts` before deciding.\n\
         - Then call exactly one of `route_to_effort` or `create_effort`.\n\
         - Do not call both.\n\
         - Titles must be short, concrete, and user-meaningful.\n\
         - Summaries should be one sentence describing what the effort is about.\n\
         - Do not answer the user directly.\n\
         - Do not mention routing, branches, or internal topology.\n"
        ,
        route_name
    )
}

fn fallback_title(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "New effort".to_string();
    }
    let words = trimmed.split_whitespace().take(6).collect::<Vec<_>>();
    if words.is_empty() {
        return "New effort".to_string();
    }
    let mut title = words.join(" ");
    title = title
        .trim_matches(|c: char| c.is_ascii_punctuation())
        .to_string();
    if title.is_empty() {
        "New effort".to_string()
    } else {
        title
    }
}

async fn create_router_runtime(
    route_name: &str,
    provider: Arc<dyn ToolProvider>,
) -> Result<LashRuntime, String> {
    let (hirsel_config, _) =
        crate::core::config::Config::load().map_err(|e| format!("failed to load config: {}", e))?;
    let provider_resolved = llm_provider::resolve_provider(&hirsel_config).await?;
    let (model, model_variant) =
        llm_provider::resolve_model_for_tier(&hirsel_config, &provider_resolved, "medium");
    let execution_mode: ExecutionMode = default_execution_mode();
    let session_policy = SessionPolicy {
        model: model.clone(),
        provider: provider_resolved,
        max_context_tokens: Some(crate::core::config::get_context_window(&model) as usize),
        model_variant,
        session_id: Some(format!("router-{}", uuid::Uuid::new_v4())),
        execution_mode,
        context_strategy: default_context_strategy(),
        ..Default::default()
    };
    let host_config = RuntimeHostConfig {
        host_profile: HostProfile::Embedded,
        prompt_overrides: vec![lash::PromptSectionOverride {
            section: lash::PromptSectionName::Guidance,
            mode: lash::PromptOverrideMode::Append,
            content: build_router_guidance(route_name),
        }],
        ..RuntimeHostConfig::default()
    };
    let plugin_host = PluginHost::new(vec![
        Arc::new(BuiltinToolResultProjectionPluginFactory::default()) as Arc<dyn PluginFactory>,
        Arc::new(StaticPluginFactory::new(
            "hirsel_router_tools",
            PluginSpec::new().with_tool_provider(provider),
        )) as Arc<dyn PluginFactory>,
    ]);
    let plugins = plugin_host
        .build_session("hirsel-router", execution_mode, None)
        .map_err(|e| format!("failed to build router tool session: {}", e))?;
    let services = RuntimeServices::new(plugins);
    let state = AgentStateEnvelope {
        agent_id: "hirsel-router".to_string(),
        policy: session_policy.clone(),
        ..AgentStateEnvelope::default()
    };
    LashRuntime::from_state(session_policy, host_config, services, state)
        .await
        .map_err(|e| format!("failed to create router runtime: {}", e))
}

async fn create_effort_for_message(
    project_id: i64,
    route_id: i64,
    title: &str,
    summary: &str,
) -> Result<ShepherdEffort, String> {
    let state = DeltaState::with_route(project_id, route_id);
    let item = state
        .create_work_item(title, None, summary, &[])
        .await
        .map_err(|e| format!("failed to create effort work item: {}", e))?;
    let _ = state
        .assign_work_item(
            &item.id,
            Some("channel"),
            Some("main"),
            Some(CapabilityProfile::Channel),
        )
        .await;
    let store = ShepherdChatStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd chat store: {}", e))?;
    store
        .create_effort(project_id, route_id, &item.id, title, summary, true)
        .await
        .map_err(|e| format!("failed to create effort: {}", e))
}

pub(super) async fn route_message_to_effort(
    project_id: i64,
    route_id: i64,
    route_name: &str,
    content: &str,
) -> Result<ShepherdEffort, String> {
    let store = ShepherdChatStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd chat store: {}", e))?;
    let efforts = store
        .list_project_efforts(project_id, route_id)
        .await
        .map_err(|e| format!("failed to load active efforts: {}", e))?;

    if efforts.is_empty() {
        return create_effort_for_message(
            project_id,
            route_id,
            &fallback_title(content),
            "New user request.",
        )
        .await;
    }

    let decision = RouterDecisionState::new();
    let provider: Arc<dyn ToolProvider> =
        Arc::new(RouterToolProvider::new(efforts.clone(), decision.clone()));
    let mut runtime = create_router_runtime(route_name, provider).await?;
    let turn = runtime
        .stream_turn(
            TurnInput {
                items: vec![InputItem::Text {
                    text: content.trim().to_string(),
                }],
                image_blobs: Default::default(),
                mode: None,
            },
            &SilentLashSink,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .map_err(|e| format!("failed to run intake router: {}", e))?;

    tracing::info!(
        tool_calls = turn.tool_calls.len(),
        "completed intake routing turn"
    );

    let Some(choice) = decision.take()? else {
        if let Some(focused) = efforts.iter().find(|effort| effort.focused) {
            store
                .set_focused_effort(project_id, &focused.id)
                .await
                .map_err(|e| format!("failed to keep focused effort: {}", e))?;
            return store
                .get_effort(&focused.id)
                .await
                .map_err(|e| format!("failed to load focused effort: {}", e));
        }
        return create_effort_for_message(
            project_id,
            route_id,
            &fallback_title(content),
            "New user request.",
        )
        .await;
    };

    match choice {
        RouterDecision::RouteTo { effort_id } => {
            store
                .set_focused_effort(project_id, &effort_id)
                .await
                .map_err(|e| format!("failed to focus effort: {}", e))?;
            store
                .get_effort(&effort_id)
                .await
                .map_err(|e| format!("failed to load routed effort: {}", e))
        }
        RouterDecision::Create { title, summary } => {
            create_effort_for_message(project_id, route_id, &title, &summary).await
        }
    }
}

pub(super) async fn focus_effort(
    project_id: i64,
    route_id: i64,
    effort_id: &str,
) -> Result<ShepherdEffort, String> {
    let store = ShepherdChatStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd chat store: {}", e))?;
    let effort = store
        .get_effort(effort_id)
        .await
        .map_err(|e| format!("failed to load effort: {}", e))?;
    if effort.project_id != project_id || effort.route_id != route_id {
        return Err("Effort does not belong to the active route".to_string());
    }
    store
        .set_focused_effort(project_id, effort_id)
        .await
        .map_err(|e| format!("failed to focus effort: {}", e))?;
    store
        .get_effort(effort_id)
        .await
        .map_err(|e| format!("failed to reload effort: {}", e))
}

pub(super) async fn focused_or_latest_effort(
    project_id: i64,
    route_id: i64,
) -> Result<Option<ShepherdEffort>, String> {
    let store = ShepherdChatStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd chat store: {}", e))?;
    if let Some(focused) = store
        .focused_effort(project_id, route_id)
        .await
        .map_err(|e| format!("failed to load focused effort: {}", e))?
    {
        return Ok(Some(focused));
    }
    let efforts = store
        .list_project_efforts(project_id, route_id)
        .await
        .map_err(|e| format!("failed to list efforts: {}", e))?;
    Ok(efforts.into_iter().next())
}
