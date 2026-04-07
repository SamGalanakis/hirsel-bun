//! Librarian — knowledge graph agent backed by SurrealDB.
//!
//! The Librarian is authenticated as a SurrealDB record-user whose table
//! permissions restrict it to `kg_node` and `kg_edge` rows belonging to its
//! project. The canvas document node (`document:canvas`) is additionally
//! protected from deletion by the `kg_node` PERMISSIONS clause.

#[cfg(feature = "host")]
use lash::ToolResult;
#[cfg(feature = "host")]
use serde_json::{json, Map, Value};

#[cfg(feature = "host")]
use crate::backend::db::{global_db, librarian_db};
#[cfg(feature = "host")]
use crate::backend::knowledge_graph::KnowledgeGraphTextRow;
#[cfg(feature = "host")]
use crate::backend::live_updates::{self, LiveUpdateKind};
#[cfg(feature = "host")]
use crate::backend::shepherd_runtime::ShepherdMessageChunk;
#[cfg(feature = "host")]
use crate::backend::text_patch::{apply_text_patch, is_safe_patch_field_name};
#[cfg(feature = "host")]
use crate::backend::tool_results::edit_result_with;

#[cfg(feature = "host")]
const GRAPH_MUTATION_KEYWORDS: &[&str] = &["CREATE", "UPSERT", "UPDATE", "DELETE", "RELATE"];
#[cfg(feature = "host")]
const GRAPH_TEXT_FIELDS: &[&str] = &["content"];

pub(crate) const LIBRARIAN_SURREALQL_GUIDE: &str = r#"## Knowledge Graph Querying

Use `graph_surql(query, params?)` for graph reads and writes. Your session is scoped to the current project — table permissions enforce project isolation automatically, so you do not need to filter by `project_id` yourself (though you may still reference `$project_id` if convenient, as it is always bound).

Allowed tables:
- `kg_node`
- `kg_edge`

Canonical node record IDs:
- `type::record('kg_node', [$project_id, 'artifact', 'src/auth.rs'])`
- `type::record('kg_node', [$project_id, 'feature', 'auth'])`
- `type::record('kg_node', [$project_id, 'document', 'canvas'])`

Node shape:
- `project_id`, `kind`, `node_id`, `label`, `content`, `source`, `metadata`, `updated_at`
- `content` is the single text field for all node kinds. For documents, it holds HTML. For everything else, plain text.

Edge shape:
- `project_id`, `relation`, `metadata`, `created_at`

Preferred patterns:
- Lookup/list: `SELECT * FROM kg_node WHERE kind = $kind AND ...`
- Upsert node: `UPSERT type::record('kg_node', [$project_id, $kind, $node_id]) MERGE { ... }`
- Relate nodes: `RELATE $from->kg_edge->$to SET project_id = $project_id, relation = 'part_of', metadata = {}`
- Multi-step updates: `BEGIN TRANSACTION; ... COMMIT TRANSACTION;`
- Abort a bad transaction: `THROW 'reason'`

Note: The canvas document node cannot be deleted.

For incremental refinement of a long `content` field, use `edit_graph_node_text(kind, id, field, patch)` instead of rewriting the whole node."#;

// ── Turn-based Librarian Trigger ──

/// Build a context message from recent shepherd/thread conversation history
/// and send it to the librarian for processing.
#[cfg(feature = "host")]
pub async fn trigger_librarian_after_turn(
    project_id: i64,
    scope_label: &str,
    summary: &str,
) -> Result<(), String> {
    // Fetch recent conversation context
    let history = crate::backend::shepherd_runtime::get_shepherd_history(
        crate::backend::shepherd_runtime::ShepherdScope::Shepherd {
            project_id,
            workspace_path: None,
            focus: None,
        },
        12,
    )
    .await?;

    let mut context = String::new();
    for msg in &history {
        append_message_context(&mut context, &msg.role, &msg.chunks_json);
    }

    let message = format!(
        "A {scope_label} turn just completed. Review the conversation below and update the knowledge graph, then refresh the canvas.\n\n\
         Turn summary: {summary}\n\n\
         Recent conversation:\n{context}",
    );

    // Send to the librarian scope
    crate::backend::shepherd_runtime::send_scope_message(
        crate::backend::shepherd_runtime::ShepherdScope::Librarian {
            project_id,
            workspace_path: None,
        },
        Some(message),
        None,
        None,
    )
    .await
    .map(|_| ())
    .map_err(|e| format!("failed to trigger librarian: {e}"))
}

#[cfg(feature = "host")]
fn append_message_context(out: &mut String, role: &str, chunks_json: &str) {
    let Ok(chunks) = serde_json::from_str::<Vec<ShepherdMessageChunk>>(chunks_json) else {
        return;
    };
    for chunk in chunks {
        let ShepherdMessageChunk::Text { content } = chunk else {
            continue;
        };
        let truncated = if content.len() > 400 {
            format!("{}...", &content[..400])
        } else {
            content.to_string()
        };
        out.push_str(&format!("  [{role}]: {truncated}\n"));
    }
}

// ── Graph Operations ──

#[cfg(feature = "host")]
pub async fn graph_surql(project_id: i64, args: &Value) -> ToolResult {
    let Some(query) = args.get("query").and_then(|value| value.as_str()) else {
        return ToolResult::err(json!({ "error": "query is required" }));
    };
    let params = match args.get("params") {
        None => Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => {
            return ToolResult::err(json!({
                "error": "params must be an object when provided"
            }));
        }
    };

    let db = match librarian_db(project_id).await {
        Ok(db) => db,
        Err(error) => {
            tracing::error!(%error, project_id, "failed to get librarian db");
            return ToolResult::err(json!({
                "error": format!("failed to get librarian db: {error}")
            }));
        }
    };

    let mut query_builder = db.query(query).bind(("project_id", project_id));
    if !params.is_empty() {
        query_builder = query_builder.bind(Value::Object(params));
    }

    let mut response = match query_builder.await {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(%error, project_id, "graph_surql query failed");
            return ToolResult::err(json!({
                "error": format!("graph_surql failed: {error}")
            }));
        }
    };

    let statement_count = response.num_statements();
    let errors = response.take_errors();
    if !errors.is_empty() {
        let msg = format_graph_errors(errors);
        tracing::warn!(project_id, error = %msg, "graph_surql statement errors");
        return ToolResult::err(json!({
            "error": msg
        }));
    }

    let mut results = Vec::with_capacity(statement_count);
    for index in 0..statement_count {
        let rows: Vec<Value> = response.take(index).unwrap_or_default();
        results.push(json!({
            "index": index,
            "value": rows,
        }));
    }

    if query_might_mutate_graph(query) {
        live_updates::publish_project(project_id, LiveUpdateKind::KnowledgeGraphChanged);
    }

    ToolResult::ok(json!({
        "statement_count": statement_count,
        "results": results,
    }))
}

#[cfg(feature = "host")]
pub async fn edit_graph_node_text(project_id: i64, args: &Value) -> ToolResult {
    let Some(kind) = args.get("kind").and_then(|value| value.as_str()) else {
        return ToolResult::err(json!({ "error": "kind is required" }));
    };
    let Some(node_id) = args.get("id").and_then(|value| value.as_str()) else {
        return ToolResult::err(json!({ "error": "id is required" }));
    };
    let Some(field) = args.get("field").and_then(|value| value.as_str()) else {
        return ToolResult::err(json!({ "error": "field is required" }));
    };
    let Some(patch) = args.get("patch").and_then(|value| value.as_str()) else {
        return ToolResult::err(json!({ "error": "patch is required" }));
    };

    if !is_allowed_graph_text_field(field) {
        return ToolResult::err(json!({
            "error": format!(
                "field must be one of: {}",
                GRAPH_TEXT_FIELDS.join(", ")
            )
        }));
    }
    if !is_safe_patch_field_name(field) {
        return ToolResult::err(json!({ "error": "field name is not safe" }));
    }

    let node_key = graph_node_record_key(project_id, kind, node_id);
    let db = global_db().await;

    let mut response = match db
        .query(
            "LET $node = type::record('kg_node', $node_key);
             SELECT content FROM $node WHERE project_id = $project_id;",
        )
        .bind(("project_id", project_id))
        .bind(("node_key", node_key.clone()))
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return ToolResult::err(json!({
                "error": format!("failed to load graph node: {error}")
            }));
        }
    };

    let record: Option<KnowledgeGraphTextRow> = match response.take(1) {
        Ok(record) => record,
        Err(error) => {
            return ToolResult::err(json!({
                "error": format!("failed to decode graph node: {error}")
            }));
        }
    };
    let Some(record) = record else {
        return ToolResult::err(json!({
            "error": format!("kg_node not found for kind='{kind}' id='{node_id}'")
        }));
    };

    let current = record.content.as_deref().unwrap_or("");

    let patched = match apply_text_patch(current, patch) {
        Ok(outcome) => outcome,
        Err(error) => return ToolResult::err(json!({ "error": error })),
    };

    let query = format!(
        "UPDATE type::record('kg_node', $node_key)
         MERGE {{
            project_id: $project_id,
            updated_at: time::now(),
            {field}: $value
          }};"
    );

    if let Err(error) = db
        .query(&query)
        .bind(("project_id", project_id))
        .bind(("node_key", node_key))
        .bind(("value", patched.new_text.clone()))
        .await
    {
        tracing::error!(%error, project_id, kind, node_id, field, "edit_graph_node_text update failed");
        return ToolResult::err(json!({
            "error": format!("failed to update graph node text: {error}")
        }));
    }

    live_updates::publish_project(project_id, LiveUpdateKind::KnowledgeGraphChanged);

    let mut fields = Map::new();
    fields.insert("project_id".to_string(), json!(project_id));
    fields.insert("kind".to_string(), json!(kind));
    fields.insert("node_id".to_string(), json!(node_id));
    fields.insert("field".to_string(), json!(field));
    fields.insert("added".to_string(), json!(patched.added_lines));
    fields.insert("removed".to_string(), json!(patched.removed_lines));

    edit_result_with(format!("Patched {field} on {kind}:{node_id}"), fields)
}

// ── Full workspace scan ──

/// Send a scan prompt to the Librarian. It uses its own workspace tools to explore.
#[cfg(feature = "host")]
pub async fn trigger_scan(project_id: i64) -> Result<(), String> {
    use crate::backend::shepherd_runtime::commands::send_scope_message;
    use crate::backend::shepherd_runtime::types::ShepherdScope;

    let scope = ShepherdScope::Librarian {
        project_id,
        workspace_path: None,
    };

    let message = "\
        Full scan. Explore the codebase, read entry points, configs, and main modules. \
        Build or refresh the knowledge graph with artifacts, features, issues, decisions, documents, and dependencies. \
        Prune stale graph state that no longer matches the workspace."
        .to_string();

    send_scope_message(scope, Some(message), None, None).await?;
    Ok(())
}

#[cfg(feature = "host")]
fn graph_node_record_key(project_id: i64, kind: &str, node_id: &str) -> Value {
    json!([project_id, kind, node_id])
}

#[cfg(feature = "host")]
fn is_allowed_graph_text_field(field: &str) -> bool {
    GRAPH_TEXT_FIELDS.contains(&field)
}

#[cfg(feature = "host")]
fn query_might_mutate_graph(query: &str) -> bool {
    let upper = query.to_uppercase();
    GRAPH_MUTATION_KEYWORDS
        .iter()
        .any(|keyword| upper.contains(keyword))
}

#[cfg(feature = "host")]
fn format_graph_errors(errors: std::collections::HashMap<usize, surrealdb::Error>) -> String {
    errors
        .into_iter()
        .map(|(index, error)| format!("statement {index}: {error}"))
        .collect::<Vec<_>>()
        .join("\n")
}
