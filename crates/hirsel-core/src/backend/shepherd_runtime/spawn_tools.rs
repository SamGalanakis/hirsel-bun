//! Shepherd tools that expose the spawn/await/inspect/merge/discard
//! lifecycle to the LM. Gated per-thread by the `spawn_thread` capability.

use std::sync::Arc;

use lash::plugin::{PluginFactory, StaticPluginFactory};
use lash::{PluginSpec, PromptContribution, ToolDefinition, ToolParam, ToolProvider, ToolResult};
use serde_json::{json, Value};

use super::spawn::{
    await_thread, discard_thread, inspect_thread, merge_thread, merge_thread_retry, spawn_thread,
    spawn_thread_batch, SpawnThreadRequest,
};

macro_rules! tool_definition {
    ($($field:tt)*) => {
        ToolDefinition {
            $($field)*
            input_schema_override: None,
            output_schema_override: None,
        }
    };
}

struct SpawnToolProvider {
    project_id: i64,
    /// The thread this provider is registered into — becomes the `parent_id`
    /// of any child spawned from it. `None` for the project-level shepherd.
    parent_thread_id: Option<String>,
}

#[async_trait::async_trait]
impl ToolProvider for SpawnToolProvider {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![
            tool_definition! {
                name: "spawn_thread".to_string(),
                description: concat!(
                    "Spawn a child thread to work on an objective autonomously. ",
                    "Returns a handle you can pass to await_thread / inspect_thread / merge_thread / discard_thread. ",
                    "If the child's capabilities include 'workspace_write', it gets a CoW copy of the workspace at its own path. ",
                    "The child's final output is its last assistant message once its turn settles."
                ).to_string(),
                params: vec![
                    ToolParam::typed("objective", "str"),
                    ToolParam::optional("title", "str"),
                    ToolParam::optional("capabilities", "list"),
                    ToolParam::optional("binding_kind", "str"),
                    ToolParam::optional("binding_data", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "spawn_thread_batch".to_string(),
                description: concat!(
                    "Spawn several child threads at once. Takes a list of spawn requests (same shape as spawn_thread's arguments). ",
                    "Returns a list of handles in input order."
                ).to_string(),
                params: vec![ToolParam::typed("requests", "list")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "await_thread".to_string(),
                description: concat!(
                    "Wait for a spawned thread's turn to finish (or until timeout_ms elapses). ",
                    "Returns {state: 'done', thread_id, final_output} or {state: 'pending', thread_id} — re-await pending handles later."
                ).to_string(),
                params: vec![
                    ToolParam::typed("thread_id", "str"),
                    ToolParam::optional("timeout_ms", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "await_threads".to_string(),
                description: "Await multiple handles. Returns a list aligned with the input ids.".to_string(),
                params: vec![
                    ToolParam::typed("thread_ids", "list"),
                    ToolParam::optional("timeout_ms", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "inspect_thread".to_string(),
                description: concat!(
                    "Structured summary of a spawned thread: status, merge_status, workspace_path, diff stats (for workspace-writing children), ",
                    "and final_output. Use this to decide whether to merge or discard."
                ).to_string(),
                params: vec![ToolParam::typed("thread_id", "str")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "merge_thread".to_string(),
                description: concat!(
                    "Merge a child thread's workspace-copy delta into the canonical workspace (git 3-way). ",
                    "On conflict, returns {state: 'conflict', files} — resolve via apply_patch in the canonical workspace, then call merge_thread_retry."
                ).to_string(),
                params: vec![ToolParam::typed("thread_id", "str")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "merge_thread_retry".to_string(),
                description: "Re-attempt a merge after resolving conflicts in the canonical workspace.".to_string(),
                params: vec![ToolParam::typed("thread_id", "str")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "discard_thread".to_string(),
                description: "Drop a child thread's workspace copy without merging. The thread row and chat history remain for audit.".to_string(),
                params: vec![ToolParam::typed("thread_id", "str")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
        ]
    }

    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        match name {
            "spawn_thread" => self.spawn(args).await,
            "spawn_thread_batch" => self.spawn_batch(args).await,
            "await_thread" => self.await_one(args).await,
            "await_threads" => self.await_many(args).await,
            "inspect_thread" => self.inspect(args).await,
            "merge_thread" => self.merge(args).await,
            "merge_thread_retry" => self.merge_retry(args).await,
            "discard_thread" => self.discard(args).await,
            _ => ToolResult::err(json!({ "error": format!("Unknown tool: {name}") })),
        }
    }
}

impl SpawnToolProvider {
    async fn spawn(&self, args: &Value) -> ToolResult {
        let request = match parse_spawn_request(args) {
            Ok(r) => r,
            Err(e) => return ToolResult::err(json!({ "error": e })),
        };
        match spawn_thread(self.project_id, self.parent_thread_id.clone(), request).await {
            Ok(spawned) => ToolResult::ok(serde_json::to_value(spawned).unwrap_or_default()),
            Err(e) => ToolResult::err(json!({ "error": e })),
        }
    }

    async fn spawn_batch(&self, args: &Value) -> ToolResult {
        let Some(requests_array) = args.get("requests").and_then(|v| v.as_array()) else {
            return ToolResult::err(json!({ "error": "requests (list) is required" }));
        };
        let mut requests = Vec::with_capacity(requests_array.len());
        for (i, r) in requests_array.iter().enumerate() {
            match parse_spawn_request(r) {
                Ok(req) => requests.push(req),
                Err(e) => {
                    return ToolResult::err(json!({
                        "error": format!("requests[{i}]: {e}")
                    }))
                }
            }
        }
        match spawn_thread_batch(self.project_id, self.parent_thread_id.clone(), requests).await {
            Ok(spawned) => ToolResult::ok(json!({ "handles": spawned })),
            Err(e) => ToolResult::err(json!({ "error": e })),
        }
    }

    async fn await_one(&self, args: &Value) -> ToolResult {
        let Some(thread_id) = args.get("thread_id").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "thread_id is required" }));
        };
        let timeout_ms = args.get("timeout_ms").and_then(|v| v.as_u64());
        match await_thread(self.project_id, thread_id.to_string(), timeout_ms).await {
            Ok(outcome) => ToolResult::ok(serde_json::to_value(outcome).unwrap_or_default()),
            Err(e) => ToolResult::err(json!({ "error": e })),
        }
    }

    async fn await_many(&self, args: &Value) -> ToolResult {
        let Some(ids) = args.get("thread_ids").and_then(|v| v.as_array()) else {
            return ToolResult::err(json!({ "error": "thread_ids (list) is required" }));
        };
        let timeout_ms = args.get("timeout_ms").and_then(|v| v.as_u64());
        let mut results = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(id_str) = id.as_str() else {
                return ToolResult::err(json!({ "error": "thread_ids must be a list of strings" }));
            };
            let outcome = await_thread(self.project_id, id_str.to_string(), timeout_ms).await;
            match outcome {
                Ok(o) => results.push(serde_json::to_value(o).unwrap_or_default()),
                Err(e) => results.push(json!({ "error": e, "thread_id": id_str })),
            }
        }
        ToolResult::ok(json!({ "results": results }))
    }

    async fn inspect(&self, args: &Value) -> ToolResult {
        let Some(thread_id) = args.get("thread_id").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "thread_id is required" }));
        };
        match inspect_thread(self.project_id, thread_id.to_string()).await {
            Ok(info) => ToolResult::ok(serde_json::to_value(info).unwrap_or_default()),
            Err(e) => ToolResult::err(json!({ "error": e })),
        }
    }

    async fn merge(&self, args: &Value) -> ToolResult {
        let Some(thread_id) = args.get("thread_id").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "thread_id is required" }));
        };
        match merge_thread(self.project_id, thread_id.to_string()).await {
            Ok(outcome) => ToolResult::ok(serde_json::to_value(outcome).unwrap_or_default()),
            Err(e) => ToolResult::err(json!({ "error": e })),
        }
    }

    async fn merge_retry(&self, args: &Value) -> ToolResult {
        let Some(thread_id) = args.get("thread_id").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "thread_id is required" }));
        };
        match merge_thread_retry(self.project_id, thread_id.to_string()).await {
            Ok(outcome) => ToolResult::ok(serde_json::to_value(outcome).unwrap_or_default()),
            Err(e) => ToolResult::err(json!({ "error": e })),
        }
    }

    async fn discard(&self, args: &Value) -> ToolResult {
        let Some(thread_id) = args.get("thread_id").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "thread_id is required" }));
        };
        match discard_thread(self.project_id, thread_id.to_string()).await {
            Ok(()) => ToolResult::ok(json!({ "discarded": true, "thread_id": thread_id })),
            Err(e) => ToolResult::err(json!({ "error": e })),
        }
    }
}

fn parse_spawn_request(v: &Value) -> Result<SpawnThreadRequest, String> {
    let objective = v
        .get("objective")
        .and_then(|o| o.as_str())
        .ok_or_else(|| "objective is required".to_string())?
        .to_string();
    let title = v
        .get("title")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());
    let capabilities = v
        .get("capabilities")
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    let binding_kind = v
        .get("binding_kind")
        .and_then(|b| b.as_str())
        .map(|s| s.to_string());
    let binding_data = v
        .get("binding_data")
        .and_then(|b| b.as_str())
        .map(|s| s.to_string());
    Ok(SpawnThreadRequest {
        objective,
        title,
        capabilities,
        binding_kind,
        binding_data,
    })
}

fn spawn_prompt_contributions() -> Vec<PromptContribution> {
    vec![PromptContribution::guidance(
        "spawn_tools",
        "Spawned Threads",
        concat!(
            "Spawn autonomous child threads with `spawn_thread(objective, capabilities?)`. ",
            "Use `spawn_thread_batch(requests)` to fan out in parallel. ",
            "Collect results with `await_thread(thread_id)` or `await_threads([id,...])`. ",
            "If you requested `workspace_write`, the child gets a CoW workspace copy — ",
            "inspect its diff with `inspect_thread`, then `merge_thread` or `discard_thread`. ",
            "On merge conflict, fix the canonical files via `apply_patch` and call `merge_thread_retry`."
        ),
    )]
}

pub(super) fn spawn_tool_plugin_factory(
    project_id: i64,
    parent_thread_id: Option<String>,
) -> Arc<dyn PluginFactory> {
    Arc::new(StaticPluginFactory::new(
        "spawn_tools",
        PluginSpec::new()
            .with_tool_provider(Arc::new(SpawnToolProvider {
                project_id,
                parent_thread_id,
            }) as Arc<dyn ToolProvider>)
            .with_prompt_contributor(Arc::new(|_ctx| {
                Box::pin(async { Ok(spawn_prompt_contributions()) })
            })),
    ))
}
