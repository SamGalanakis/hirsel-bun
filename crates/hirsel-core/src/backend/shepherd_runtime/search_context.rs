use std::collections::HashMap;
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
use crate::backend::embeddings::EmbeddingClient;
use crate::backend::librarian::query_might_mutate_graph;
use crate::backend::llm_provider::{self, RuntimeModelRole};
use crate::backend::runtime_settings::{keys, Defaults, RuntimeSettings};

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
        let mut pairs: Vec<(String, String)> = Vec::new();
        for row in rows {
            if let (Some(kind), Some(node_id)) = (
                row.get("kind").and_then(|v| v.as_str()),
                row.get("node_id").and_then(|v| v.as_str()),
            ) {
                node_keys.push(json!([self.project_id, kind, node_id]));
                pairs.push((kind.to_string(), node_id.to_string()));
            }
        }
        if node_keys.is_empty() {
            return;
        }
        let project_id = self.project_id;
        let pairs_clone = pairs.clone();
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
        // Append implicit read-marks so other threads can see who touched
        // these nodes. Thread_id is not available here (search_context runs
        // inside a short-lived sub-runtime), so we log project-scoped reads.
        tokio::spawn(async move {
            if let Ok(store) = crate::backend::kg_read::ReadStore::open().await {
                store
                    .record_batch(project_id, &pairs_clone, None, None)
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

        match hybrid_search(self.project_id, query).await {
            Ok(HybridOutcome::Hybrid { results, mode }) => {
                let rows: Vec<Value> = results
                    .iter()
                    .map(|r| {
                        json!({
                            "kind": r.kind,
                            "node_id": r.node_id,
                            "label": r.label,
                            "content": r.content,
                            "score": r.score,
                            "snippets": r.snippets,
                        })
                    })
                    .collect();
                self.bump_read_timestamps(&rows);
                ToolResult::ok(json!({
                    "query": query,
                    "mode": mode,
                    "results": rows,
                }))
            }
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }
}

// ── Hybrid retrieval (BM25 over fused_text + HNSW over embedding, RRF fused) ──

#[derive(Debug, Clone)]
struct HybridHit {
    kind: String,
    node_id: String,
    label: String,
    content: Option<String>,
    score: f64,
    snippets: Vec<Value>,
}

enum HybridOutcome {
    Hybrid {
        results: Vec<HybridHit>,
        mode: &'static str,
    },
}

async fn hybrid_search(project_id: i64, query: &str) -> Result<HybridOutcome, String> {
    let db = librarian_db(project_id)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let rrf_k = RuntimeSettings::get_or(keys::RETRIEVAL_RRF_K, Defaults::RETRIEVAL_RRF_K).await;
    let w_lex = RuntimeSettings::get_or(
        keys::RETRIEVAL_LEXICAL_WEIGHT,
        Defaults::RETRIEVAL_LEXICAL_WEIGHT,
    )
    .await;
    let w_vec = RuntimeSettings::get_or(
        keys::RETRIEVAL_VECTOR_WEIGHT,
        Defaults::RETRIEVAL_VECTOR_WEIGHT,
    )
    .await;
    let candidate_limit = RuntimeSettings::get_or(
        keys::RETRIEVAL_CANDIDATE_LIMIT,
        Defaults::RETRIEVAL_CANDIDATE_LIMIT,
    )
    .await
    .max(1);
    let final_limit =
        RuntimeSettings::get_or(keys::RETRIEVAL_FINAL_LIMIT, Defaults::RETRIEVAL_FINAL_LIMIT)
            .await
            .max(1);
    let chunks_per_node = RuntimeSettings::get_or(
        keys::RETRIEVAL_CHUNKS_PER_NODE,
        Defaults::RETRIEVAL_CHUNKS_PER_NODE,
    )
    .await
    .max(1);

    let lex_rows: Vec<Value> = db
        .query(
            "SELECT node_kind, node_id, chunk_index, chunk_path, chunk_text, context_text, \
               search::score(1) AS lex_score \
             FROM kg_node_chunk \
             WHERE project_id = $pid AND fused_text @1@ $query \
             ORDER BY lex_score DESC LIMIT $limit",
        )
        .bind(("pid", project_id))
        .bind(("query", query.to_string()))
        .bind(("limit", candidate_limit as i64))
        .await
        .map_err(|e| format!("lexical chunk query: {e}"))?
        .take(0)
        .unwrap_or_default();

    // Try the vector branch. If OpenRouter isn't configured (or the embed
    // call fails), run the lexical-only branch. If lexical also returned
    // nothing, fall back to BM25 over kg_node itself so old projects
    // without any chunks still see something.
    let vec_rows = match EmbeddingClient::from_credentials().await {
        Ok(client) => match client.embed_query(query).await {
            Ok(vec) => db
                .query(
                    "SELECT node_kind, node_id, chunk_index, chunk_path, chunk_text, context_text, \
                       vector::distance::knn() AS dist \
                     FROM kg_node_chunk \
                     WHERE project_id = $pid AND embedding <|$limit, COSINE|> $q \
                     ORDER BY dist ASC LIMIT $limit",
                )
                .bind(("pid", project_id))
                .bind(("q", vec))
                .bind(("limit", candidate_limit as i64))
                .await
                .map_err(|e| format!("vector chunk query: {e}"))?
                .take(0)
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    };

    if lex_rows.is_empty() && vec_rows.is_empty() {
        // No chunks at all for this project — fall back to BM25 on kg_node.
        return legacy_bm25_search(project_id, query, final_limit).await;
    }

    let mode: &'static str = if vec_rows.is_empty() {
        "lexical_chunks"
    } else if lex_rows.is_empty() {
        "vector_chunks"
    } else {
        "hybrid"
    };

    // Per-chunk RRF scoring.
    #[derive(Default)]
    struct NodeAccum {
        score: f64,
        snippets: Vec<(f64, Value)>, // (score, row-as-snippet)
    }
    let mut per_node: HashMap<(String, String), NodeAccum> = HashMap::new();

    let add_ranked =
        |rows: &[Value], weight: f64, per_node: &mut HashMap<(String, String), NodeAccum>| {
            for (rank, row) in rows.iter().enumerate() {
                let Some(kind) = row.get("node_kind").and_then(|v| v.as_str()) else {
                    continue;
                };
                let Some(node_id) = row.get("node_id").and_then(|v| v.as_str()) else {
                    continue;
                };
                let score = weight / (rrf_k + (rank as f64) + 1.0);
                let entry = per_node
                    .entry((kind.to_string(), node_id.to_string()))
                    .or_default();
                entry.score += score;
                entry.snippets.push((score, row.clone()));
            }
        };
    add_ranked(&lex_rows, w_lex, &mut per_node);
    add_ranked(&vec_rows, w_vec, &mut per_node);

    let mut ranked: Vec<((String, String), NodeAccum)> = per_node.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.score
            .partial_cmp(&a.1.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked.truncate(final_limit);

    // Fetch the canonical node rows for the finalists, keeping only non-
    // superseded nodes.
    let mut results: Vec<HybridHit> = Vec::with_capacity(ranked.len());
    for ((kind, node_id), mut accum) in ranked {
        let key = json!([project_id, kind, node_id]);
        let Ok(mut response) = db
            .query(
                "SELECT kind, node_id, label, content FROM type::record('kg_node', $key) \
                 WHERE superseded_at = NONE",
            )
            .bind(("key", key))
            .await
        else {
            continue;
        };
        let rows: Vec<Value> = response.take(0).unwrap_or_default();
        let Some(row) = rows.into_iter().next() else {
            continue;
        };
        let label = row
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let content = row
            .get("content")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        // Deduplicate snippets by chunk_index, keep best-scoring first.
        accum
            .snippets
            .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut seen_idx: std::collections::BTreeSet<i64> = Default::default();
        let mut snippets: Vec<Value> = Vec::new();
        for (_score, snip) in accum.snippets.into_iter() {
            let idx = snip
                .get("chunk_index")
                .and_then(|v| v.as_i64())
                .unwrap_or(-1);
            if idx >= 0 && !seen_idx.insert(idx) {
                continue;
            }
            snippets.push(json!({
                "chunk_index": idx,
                "chunk_path": snip.get("chunk_path").cloned().unwrap_or(json!([])),
                "chunk_text": snip.get("chunk_text").cloned().unwrap_or(json!("")),
                "context_text": snip.get("context_text").cloned().unwrap_or(json!("")),
            }));
            if snippets.len() >= chunks_per_node {
                break;
            }
        }
        results.push(HybridHit {
            kind,
            node_id,
            label,
            content,
            score: accum.score,
            snippets,
        });
    }

    Ok(HybridOutcome::Hybrid { results, mode })
}

/// BM25 fallback for projects whose chunk table is empty. Same shape as
/// hybrid so the caller doesn't branch on mode.
async fn legacy_bm25_search(
    project_id: i64,
    query: &str,
    final_limit: usize,
) -> Result<HybridOutcome, String> {
    let db = librarian_db(project_id)
        .await
        .map_err(|e| format!("DB error: {e}"))?;
    let rows: Vec<Value> = db
        .query(
            "SELECT kind, node_id, label, content, \
               search::score(1) + search::score(2) AS score \
             FROM kg_node \
             WHERE (label @1@ $query OR content @2@ $query) \
               AND superseded_at = NONE \
             ORDER BY score DESC LIMIT $limit",
        )
        .bind(("query", query.to_string()))
        .bind(("limit", final_limit as i64))
        .await
        .map_err(|e| format!("legacy BM25 search: {e}"))?
        .take(0)
        .unwrap_or_default();

    let results: Vec<HybridHit> = rows
        .into_iter()
        .map(|row| HybridHit {
            kind: row
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            node_id: row
                .get("node_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            label: row
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            content: row
                .get("content")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            score: row.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0),
            snippets: Vec::new(),
        })
        .collect();

    Ok(HybridOutcome::Hybrid {
        results,
        mode: "legacy_bm25",
    })
}

// ── Sub-agent execution ──

const SEARCH_AGENT_SYSTEM_PROMPT: &str = "\
You are a knowledge graph search agent. Answer the question using the graph tools.

The graph contains nodes with kinds: component, entity, convention, decision, fact, goal, document.
Each node has: kind, node_id, label, content, tags (array), source, metadata.
Edges have: relation (part_of, depends_on, implements, relates_to).

`search_graph_text` returns hybrid retrieval results: each hit carries a node plus
the most relevant chunk snippets (chunk_text + context_text) drawn from its content.
Use the snippets to decide which nodes to cite; re-read the full node via `graph_query`
when you need surrounding context. The `mode` field tells you which branches fired
(hybrid | lexical_chunks | vector_chunks | legacy_bm25).

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
