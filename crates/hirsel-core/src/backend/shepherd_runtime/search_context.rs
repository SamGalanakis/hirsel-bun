use std::sync::Arc;
use std::time::Duration;

use lash::plugin::{PluginFactory, StaticPluginFactory};
use lash::{
    default_execution_mode, EventSink, HostProfile, InputItem, LashRuntime, PluginHost, PluginSpec,
    PromptContribution, PromptOverrideMode, PromptSectionName, PromptSectionOverride,
    RuntimeHostConfig, SessionEvent, SessionPolicy, SessionStateEnvelope, ToolDefinition,
    ToolParam, ToolProvider, ToolResult, TurnInput,
};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::backend::db::librarian_db;
use crate::backend::librarian::query_might_mutate_graph;
use crate::backend::llm_provider::{self, RuntimeModelRole};

// ── Collecting event sink (captures text output, discards everything else) ──

struct CollectingEventSink {
    text: tokio::sync::Mutex<String>,
}

#[async_trait::async_trait]
impl EventSink for CollectingEventSink {
    async fn emit(&self, event: SessionEvent) {
        if let SessionEvent::TextDelta { content } = event {
            self.text.lock().await.push_str(&content);
        }
    }
}

// ── Read-only graph tool provider (tools for the search sub-agent) ──

struct ReadOnlyGraphToolProvider {
    project_id: i64,
}

macro_rules! tool_definition {
    ($($field:tt)*) => {
        ToolDefinition {
            $($field)*
            input_schema_override: None,
            output_schema_override: None,
        }
    };
}

#[async_trait::async_trait]
impl ToolProvider for ReadOnlyGraphToolProvider {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![
            tool_definition! {
                name: "graph_query".to_string(),
                description: "Run a read-only SurrealQL query against the project knowledge graph. `$project_id` is bound automatically. Mutations are rejected.".to_string(),
                params: vec![
                    ToolParam::typed("query", "str"),
                    ToolParam::optional("params", "dict"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "search_graph_text".to_string(),
                description: "Full-text search over knowledge graph node labels and content. Returns matching nodes ranked by relevance.".to_string(),
                params: vec![
                    ToolParam::typed("query", "str"),
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
            "graph_query" => self.graph_query(args).await,
            "search_graph_text" => self.search_graph_text(args).await,
            _ => ToolResult::err(json!({ "error": format!("Unknown tool: {name}") })),
        }
    }
}

impl ReadOnlyGraphToolProvider {
    fn bump_read_timestamps(&self, rows: &[Value]) {
        let mut node_keys: Vec<Value> = Vec::new();
        for row in rows {
            if let (Some(kind), Some(node_id)) = (
                row.get("kind").and_then(|v| v.as_str()),
                row.get("node_id").and_then(|v| v.as_str()),
            ) {
                node_keys.push(json!([self.project_id, kind, node_id]));
            }
        }
        if node_keys.is_empty() {
            return;
        }
        let project_id = self.project_id;
        tokio::spawn(async move {
            let Ok(db) = librarian_db(project_id).await else {
                return;
            };
            for key in node_keys {
                let _ = db
                    .query("UPDATE type::record('kg_node', $key) SET read_by_search_context = time::now()")
                    .bind(("key", key))
                    .await;
            }
        });
    }

    async fn graph_query(&self, args: &Value) -> ToolResult {
        let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "query is required" }));
        };

        if query_might_mutate_graph(query) {
            return ToolResult::err(json!({
                "error": "This is a read-only tool. Mutations (CREATE, UPSERT, UPDATE, DELETE, RELATE, INSERT) are not allowed."
            }));
        }

        let params = match args.get("params") {
            None => serde_json::Map::new(),
            Some(Value::Object(map)) => map.clone(),
            Some(_) => {
                return ToolResult::err(json!({ "error": "params must be an object" }));
            }
        };

        let db = match librarian_db(self.project_id).await {
            Ok(db) => db,
            Err(error) => {
                return ToolResult::err(json!({ "error": format!("DB error: {error}") }));
            }
        };

        let mut query_builder = db.query(query).bind(("project_id", self.project_id));
        if !params.is_empty() {
            query_builder = query_builder.bind(Value::Object(params));
        }

        let mut response = match query_builder.await {
            Ok(r) => r,
            Err(error) => {
                return ToolResult::err(json!({ "error": format!("Query failed: {error}") }));
            }
        };

        let errors = response.take_errors();
        if !errors.is_empty() {
            let msg: String = errors
                .into_values()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            return ToolResult::err(json!({ "error": msg }));
        }

        let statement_count = response.num_statements();
        let mut results = Vec::with_capacity(statement_count);
        for index in 0..statement_count {
            let rows: Vec<Value> = response.take(index).unwrap_or_default();
            results.push(json!({
                "index": index,
                "row_count": rows.len(),
                "value": rows,
            }));
        }

        // Best-effort: bump read_by_search_context on returned nodes
        let all_rows: Vec<Value> = results
            .iter()
            .filter_map(|r| r.get("value"))
            .filter_map(|v| v.as_array())
            .flatten()
            .cloned()
            .collect();
        self.bump_read_timestamps(&all_rows);

        ToolResult::ok(json!({
            "statement_count": statement_count,
            "results": results,
        }))
    }

    async fn search_graph_text(&self, args: &Value) -> ToolResult {
        let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "query is required" }));
        };
        let query_owned = query.to_string();

        let db = match librarian_db(self.project_id).await {
            Ok(db) => db,
            Err(error) => {
                return ToolResult::err(json!({ "error": format!("DB error: {error}") }));
            }
        };

        let mut response = match db
            .query(
                "SELECT kind, node_id, label, content, \
                 search::score(1) + search::score(2) AS score \
                 FROM kg_node \
                 WHERE label @1@ $query OR content @2@ $query \
                 ORDER BY score DESC LIMIT 20",
            )
            .bind(("query", query_owned))
            .await
        {
            Ok(r) => r,
            Err(error) => {
                return ToolResult::err(json!({ "error": format!("Search failed: {error}") }));
            }
        };

        let rows: Vec<Value> = response.take(0).unwrap_or_default();
        self.bump_read_timestamps(&rows);
        ToolResult::ok(json!({
            "query": query,
            "results": rows,
        }))
    }
}

// ── Sub-agent execution ──

const SEARCH_AGENT_SYSTEM_PROMPT: &str = "\
You are a knowledge graph search agent. Answer the question using the graph tools.

The graph contains nodes with kinds: component, entity, convention, decision, fact, goal, document.
Each node has: kind, node_id, label, content, tags (array), source, metadata.
Edges have: relation (part_of, depends_on, implements, relates_to).

Start by reading the project index to understand what is in the graph:
  graph_query: SELECT content FROM type::record('kg_node', [$project_id, 'document', 'index'])
Then use `search_graph_text` for broad discovery and `graph_query` for targeted lookups.
Cite relevant nodes as [kind:id] in your answer. Be concise and factual.
If no relevant information exists in the graph, say so.";

async fn execute_search_context(project_id: i64, question: &str) -> ToolResult {
    let settings = match load_llm_settings().await {
        Ok(s) => s,
        Err(error) => return ToolResult::err(json!({ "error": error })),
    };
    let provider = match llm_provider::resolve_provider(&settings).await {
        Ok(p) => p,
        Err(error) => return ToolResult::err(json!({ "error": error })),
    };
    let (model, model_variant) =
        llm_provider::resolve_model_for_role(&settings, &provider, RuntimeModelRole::Search);
    let execution_mode = default_execution_mode();

    let session_policy = SessionPolicy {
        model: model.clone(),
        provider,
        max_context_tokens: Some(crate::backend::config::get_context_window(&model) as usize),
        model_variant,
        session_id: Some(format!("search-context-{}", project_id)),
        execution_mode,
        ..Default::default()
    };

    let host_config = RuntimeHostConfig {
        host_profile: HostProfile::Embedded,
        prompt_overrides: vec![PromptSectionOverride {
            section: PromptSectionName::Guidance,
            block: None,
            mode: PromptOverrideMode::Append,
            content: SEARCH_AGENT_SYSTEM_PROMPT.to_string(),
        }],
        ..RuntimeHostConfig::default()
    };

    let graph_tools: Arc<dyn ToolProvider> = Arc::new(ReadOnlyGraphToolProvider { project_id });
    let plugin_factories: Vec<Arc<dyn PluginFactory>> = vec![
        Arc::new(lash::BuiltinToolResultProjectionPluginFactory::default()),
        Arc::new(StaticPluginFactory::new(
            "search_graph",
            PluginSpec::new().with_tool_provider(graph_tools),
        )),
    ];
    let plugin_host = PluginHost::new(plugin_factories);
    let root_plugins =
        match plugin_host.build_session(format!("search-{project_id}"), execution_mode, None) {
            Ok(p) => p,
            Err(error) => {
                return ToolResult::err(json!({
                    "error": format!("Failed to build search session: {error}")
                }));
            }
        };

    let services = lash::RuntimeServices::new(root_plugins);
    let state = SessionStateEnvelope {
        session_id: format!("search-context-{project_id}"),
        policy: session_policy.clone(),
        ..SessionStateEnvelope::default()
    };

    let mut runtime =
        match LashRuntime::from_state(session_policy, host_config, services, state).await {
            Ok(r) => r,
            Err(error) => {
                return ToolResult::err(json!({
                    "error": format!("Failed to create search runtime: {error}")
                }));
            }
        };

    let sink = CollectingEventSink {
        text: tokio::sync::Mutex::new(String::new()),
    };
    let cancel = CancellationToken::new();
    let turn_input = TurnInput {
        items: vec![InputItem::Text {
            text: question.to_string(),
        }],
        image_blobs: Default::default(),
        mode: None,
        user_input: None,
    };

    let result = tokio::time::timeout(
        Duration::from_secs(60),
        runtime.stream_turn(turn_input, &sink, cancel),
    )
    .await;

    match result {
        Ok(Ok(turn)) => {
            let answer = turn.assistant_output.safe_text.trim().to_string();
            if answer.is_empty() {
                ToolResult::ok(json!({
                    "question": question,
                    "answer": "No answer produced by the search agent.",
                }))
            } else {
                ToolResult::ok(json!({
                    "question": question,
                    "answer": answer,
                }))
            }
        }
        Ok(Err(error)) => ToolResult::err(json!({
            "error": format!("Search agent failed: {error}")
        })),
        Err(_) => ToolResult::err(json!({
            "error": "Search agent timed out after 60 seconds"
        })),
    }
}

async fn load_llm_settings() -> Result<crate::backend::app_settings::LlmSettings, String> {
    let store = crate::backend::AppSettingsStore::open()
        .await
        .map_err(|e| format!("failed to open app settings store: {e}"))?;
    store
        .load_llm_settings()
        .await
        .map_err(|e| format!("failed to load llm settings: {e}"))
}

// ── Tool provider exposed to shepherd/thread scopes ──

struct SearchContextToolProvider {
    project_id: i64,
}

#[async_trait::async_trait]
impl ToolProvider for SearchContextToolProvider {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![tool_definition! {
            name: "search_context".to_string(),
            description: "Search the project's knowledge graph by asking a natural-language question. A specialized agent queries the graph and returns an answer with node citations [kind:id].".to_string(),
            params: vec![ToolParam::typed("question", "str")],
            returns: "dict".to_string(),
            examples: vec![],
            enabled: true,
            injected: true,
        }]
    }

    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        if name != "search_context" {
            return ToolResult::err(json!({ "error": format!("Unknown tool: {name}") }));
        }
        let Some(question) = args.get("question").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "question is required" }));
        };
        execute_search_context(self.project_id, question).await
    }
}

// ── Plugin factory ──

fn search_context_prompt_contributions() -> Vec<PromptContribution> {
    vec![PromptContribution::guidance(
        "search_context",
        "Knowledge Graph Search",
        "Use `search_context(question)` to query the project knowledge graph with a natural-language question. A short-lived search agent reads the graph and returns an answer with [kind:id] citations. Use it to recall project decisions, architecture, features, issues, or any previously recorded knowledge.",
    )]
}

pub(super) fn search_context_plugin_factory(project_id: i64) -> Arc<dyn PluginFactory> {
    Arc::new(StaticPluginFactory::new(
        "search_context",
        PluginSpec::new()
            .with_tool_provider(
                Arc::new(SearchContextToolProvider { project_id }) as Arc<dyn ToolProvider>
            )
            .with_prompt_contributor(Arc::new(|_ctx| {
                Box::pin(async { Ok(search_context_prompt_contributions()) })
            })),
    ))
}
