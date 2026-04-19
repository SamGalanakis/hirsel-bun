//! Shepherd tools that expose KG comments + subgraph chunking to the LM.
//! `graph.comment` / `graph.comments` / `graph.resolve` / `graph.chunk_subgraph`.
//!
//! Every `graph.comment` insert is additive — no stomping. Only the
//! comment's own author (by thread_id or "user") may resolve.

use std::sync::Arc;

use lash::plugin::{PluginFactory, StaticPluginFactory};
use lash::{PluginSpec, PromptContribution, ToolDefinition, ToolParam, ToolProvider, ToolResult};
use serde_json::{json, Value};

use crate::backend::kg_chunk;
use crate::backend::kg_comment::{Comment, CommentStore, CommentTarget};
use crate::backend::runtime_settings::{keys, Defaults, RuntimeSettings};

macro_rules! tool_definition {
    ($($field:tt)*) => {
        ToolDefinition {
            $($field)*
            input_schema_override: None,
            output_schema_override: None,
        }
    };
}

struct CommentToolProvider {
    project_id: i64,
    /// Author identity for `graph.comment`: the owning thread's id, or
    /// `"shepherd"` when the shepherd scope calls directly.
    author: String,
}

#[async_trait::async_trait]
impl ToolProvider for CommentToolProvider {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![
            tool_definition! {
                name: "graph.comment".to_string(),
                description: concat!(
                    "Add a comment to a knowledge-graph node. Comments are additive — never replace each other. ",
                    "Pass `target` with `{property?, line_start?, line_end?}` to anchor the comment to a property ",
                    "or line span within a text field; omit for whole-node."
                ).to_string(),
                params: vec![
                    ToolParam::typed("kind", "str"),
                    ToolParam::typed("node_id", "str"),
                    ToolParam::typed("body", "str"),
                    ToolParam::optional("target", "dict"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "graph.comments".to_string(),
                description: concat!(
                    "List recent comments on a node. Default filters to unresolved. ",
                    "Use `limit` to cap (default from settings)."
                ).to_string(),
                params: vec![
                    ToolParam::typed("kind", "str"),
                    ToolParam::typed("node_id", "str"),
                    ToolParam::optional("limit", "int"),
                    ToolParam::optional("only_unresolved", "bool"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "graph.resolve".to_string(),
                description: "Mark a comment resolved (own comments only; the user can resolve anyone's).".to_string(),
                params: vec![ToolParam::typed("comment_id", "str")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "graph.chunk_subgraph".to_string(),
                description: concat!(
                    "Split the subgraph rooted at a node into coherent token-bounded chunks. ",
                    "Returns a list of chunks with nodes, within-chunk edges, and cross-chunk references ",
                    "(so consumers know there's more context next door). Use this to decompose a large ",
                    "knowledge-graph neighbourhood too big for one context window — then spawn_thread_batch ",
                    "one child per chunk and reduce their outputs."
                ).to_string(),
                params: vec![
                    ToolParam::typed("kind", "str"),
                    ToolParam::typed("node_id", "str"),
                    ToolParam::optional("max_tokens", "int"),
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
            "graph.comment" => self.add(args).await,
            "graph.comments" => self.list(args).await,
            "graph.resolve" => self.resolve(args).await,
            "graph.chunk_subgraph" => self.chunk(args).await,
            _ => ToolResult::err(json!({ "error": format!("Unknown tool: {name}") })),
        }
    }
}

impl CommentToolProvider {
    async fn add(&self, args: &Value) -> ToolResult {
        let Some(kind) = args.get("kind").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "kind is required" }));
        };
        let Some(node_id) = args.get("node_id").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "node_id is required" }));
        };
        let Some(body) = args.get("body").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "body is required" }));
        };
        let target = match args.get("target") {
            Some(v) if !v.is_null() => match serde_json::from_value::<CommentTarget>(v.clone()) {
                Ok(t) => Some(t),
                Err(e) => {
                    return ToolResult::err(json!({ "error": format!("invalid target: {e}") }))
                }
            },
            _ => None,
        };
        let store = match CommentStore::open().await {
            Ok(s) => s,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };
        match store
            .add(self.project_id, kind, node_id, body, &self.author, target)
            .await
        {
            Ok(comment) => ToolResult::ok(comment_to_json(&comment)),
            Err(e) => ToolResult::err(json!({ "error": e.to_string() })),
        }
    }

    async fn list(&self, args: &Value) -> ToolResult {
        let Some(kind) = args.get("kind").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "kind is required" }));
        };
        let Some(node_id) = args.get("node_id").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "node_id is required" }));
        };
        let only_unresolved = args
            .get("only_unresolved")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let effective_limit = RuntimeSettings::resolve(
            keys::GRAPH_COMMENT_DISPLAY_LIMIT,
            limit,
            Defaults::GRAPH_COMMENT_DISPLAY_LIMIT,
        )
        .await;
        let store = match CommentStore::open().await {
            Ok(s) => s,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };
        match store
            .list_for_node(
                self.project_id,
                kind,
                node_id,
                effective_limit,
                only_unresolved,
            )
            .await
        {
            Ok(comments) => ToolResult::ok(json!({
                "comments": comments.iter().map(comment_to_json).collect::<Vec<_>>(),
            })),
            Err(e) => ToolResult::err(json!({ "error": e.to_string() })),
        }
    }

    async fn resolve(&self, args: &Value) -> ToolResult {
        let Some(id) = args.get("comment_id").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "comment_id is required" }));
        };
        let store = match CommentStore::open().await {
            Ok(s) => s,
            Err(e) => return ToolResult::err(json!({ "error": e.to_string() })),
        };
        match store.resolve(id, &self.author).await {
            Ok(comment) => ToolResult::ok(comment_to_json(&comment)),
            Err(e) => ToolResult::err(json!({ "error": e.to_string() })),
        }
    }

    async fn chunk(&self, args: &Value) -> ToolResult {
        let Some(kind) = args.get("kind").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "kind is required" }));
        };
        let Some(node_id) = args.get("node_id").and_then(|v| v.as_str()) else {
            return ToolResult::err(json!({ "error": "node_id is required" }));
        };
        let max_tokens = args
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        match kg_chunk::chunk_subgraph(self.project_id, kind, node_id, max_tokens).await {
            Ok(chunks) => {
                let total_nodes: usize = chunks.iter().map(|c| c.nodes.len()).sum();
                ToolResult::ok(json!({
                    "chunks": chunks,
                    "count": chunks.len(),
                    "total_nodes": total_nodes,
                }))
            }
            Err(e) => ToolResult::err(json!({ "error": e.to_string() })),
        }
    }
}

fn comment_to_json(c: &Comment) -> Value {
    json!({
        "id": c.id,
        "kind": c.node_kind,
        "node_id": c.node_id,
        "target": c.target,
        "body": c.body,
        "author": c.author,
        "posted_at": c.posted_at,
        "resolved_at": c.resolved_at,
    })
}

fn prompt_contributions() -> Vec<PromptContribution> {
    vec![PromptContribution::guidance(
        "graph_comments",
        "Graph Comments",
        concat!(
            "Leave additive reviews on knowledge-graph nodes with `graph.comment(kind, node_id, body, target?)`. ",
            "Target optional: `{property, line_start, line_end}` to anchor to a property or line span, or omit for whole-node. ",
            "Read recent unresolved reviews with `graph.comments(kind, node_id)`. ",
            "Resolve your own comments once addressed with `graph.resolve(comment_id)`."
        ),
    )]
}

pub(super) fn comment_tool_plugin_factory(
    project_id: i64,
    author: String,
) -> Arc<dyn PluginFactory> {
    Arc::new(StaticPluginFactory::new(
        "graph_comments",
        PluginSpec::new()
            .with_tool_provider(
                Arc::new(CommentToolProvider { project_id, author }) as Arc<dyn ToolProvider>
            )
            .with_prompt_contributor(Arc::new(|_ctx| {
                Box::pin(async { Ok(prompt_contributions()) })
            })),
    ))
}
