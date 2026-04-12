//! Librarian — knowledge graph agent backed by SurrealDB.
//!
//! The Librarian is authenticated as a SurrealDB record-user whose table
//! permissions restrict it to `kg_node` and `kg_edge` rows belonging to its
//! project. The canvas document node (`document:canvas`) is additionally
//! protected from deletion by the `kg_node` PERMISSIONS clause.

use lash::ToolResult;
use serde_json::{json, Map, Value};

use crate::backend::db::librarian_db;
use crate::backend::documents::{self, DocumentValidationError};
use crate::backend::knowledge_graph::KnowledgeGraphTextRow;
use crate::backend::live_updates::{self, LiveUpdateKind};
use crate::backend::prompts;
use crate::backend::shepherd_runtime::{ShepherdMessageChunk, ShepherdScope};
use crate::backend::text_patch::{apply_text_patch, is_safe_patch_field_name};
use crate::backend::tool_results::edit_result_with;

const GRAPH_MUTATION_KEYWORDS: &[&str] =
    &["CREATE", "UPSERT", "UPDATE", "DELETE", "RELATE", "INSERT"];
const GRAPH_TEXT_FIELDS: &[&str] = &["content"];
const CANVAS_TAGS_REQUIRING_NODE_ATTR: &[&str] = &[
    "hirsel-node-ref",
    "hirsel-node-field",
    "hirsel-node-list",
    "hirsel-doc-target",
    "hirsel-doc-link",
    "hirsel-doc-embed",
];

pub(crate) const LIBRARIAN_SURREALQL_GUIDE: &str = r#"## Knowledge Graph Querying

Use `graph_surql(query, params?)` for graph reads and writes. `$project_id` is always bound automatically.

Use `patch_canvas_document(patch)` for canvas updates.

Allowed tables:
- `kg_node`
- `kg_edge`

Canonical node record IDs:
- `type::record('kg_node', [$project_id, 'artifact', 'src/auth.rs'])`
- `type::record('kg_node', [$project_id, 'feature', 'auth'])`
- `type::record('kg_node', [$project_id, 'document', 'canvas'])`

Node shape:
- `kind`, `node_id`, `label`, `content`, `source`, `metadata`, `updated_at` (auto)
- `content` is the single text field for all node kinds. For documents, it holds HTML. For everything else, plain text.

Edge shape:
- `relation`, `metadata`, `created_at` (auto)

Preferred patterns:
- Lookup/list: `SELECT * FROM kg_node WHERE kind = $kind AND ...`
- Upsert node: `UPSERT type::record('kg_node', [$project_id, $kind, $node_id]) MERGE { kind: $kind, node_id: $node_id, label: '...', content: '...', source: 'shepherd', metadata: {} }`
- Relate nodes: `RELATE $from->kg_edge->$to SET relation = 'part_of', metadata = {}`
- Multi-step updates: `BEGIN TRANSACTION; ... COMMIT TRANSACTION;`
- Abort a bad transaction: `THROW 'reason'`

Use `UPSERT` when a node may already exist.

Canvas references must use `node="kind:id"` attributes. Do not emit separate `kind=` / `id=` attributes or `path=` links.

For incremental refinement of a long `content` field, use `edit_graph_node_text(kind, id, field, patch)` instead of rewriting the whole node."#;

// ── Librarian Sync Messages ──

fn sync_source_label(scope: &ShepherdScope) -> String {
    match scope {
        ShepherdScope::Shepherd { .. } => "Shepherd".to_string(),
        ShepherdScope::Thread {
            title, thread_id, ..
        } => {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                format!("Thread: {thread_id}")
            } else {
                format!("Thread: {trimmed}")
            }
        }
        ShepherdScope::Librarian { .. } => "Librarian".to_string(),
        ShepherdScope::General => "General".to_string(),
    }
}

fn format_sync_chunks(chunks: &[ShepherdMessageChunk]) -> String {
    let mut lines = Vec::new();
    let mut image_count = 0usize;

    for chunk in chunks {
        match chunk {
            ShepherdMessageChunk::Text { content } => {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    lines.push(trimmed.to_string());
                }
            }
            ShepherdMessageChunk::Notice { title, content, .. } => {
                let trimmed = content.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let prefix = title
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| format!("[{value}] "))
                    .unwrap_or_default();
                lines.push(format!("{prefix}{trimmed}"));
            }
            ShepherdMessageChunk::Tool { .. } => {}
            ShepherdMessageChunk::Image { .. } => {
                image_count += 1;
            }
            ShepherdMessageChunk::Skill { .. } => {}
            ShepherdMessageChunk::FileRef { .. } => {}
            ShepherdMessageChunk::Thinking { .. } => {}
        }
    }

    if image_count > 0 {
        lines.push(format!(
            "[{} image attachment{}]",
            image_count,
            if image_count == 1 { "" } else { "s" }
        ));
    }

    let joined = lines.join("\n\n");
    if joined.trim().is_empty() {
        "[No user-visible content]".to_string()
    } else {
        joined
    }
}

pub async fn queue_background_sync(
    project_id: i64,
    source_scope: &ShepherdScope,
    user_chunks: &[ShepherdMessageChunk],
    assistant_chunks: &[ShepherdMessageChunk],
) -> Result<(), String> {
    let source_label = sync_source_label(source_scope);
    let user_message = format_sync_chunks(user_chunks);
    let assistant_message = format_sync_chunks(assistant_chunks);
    let prompt = prompts::render_librarian_sync(&source_label, &user_message, &assistant_message)?;

    crate::backend::shepherd_runtime::commands::enqueue_librarian_automated_message(
        project_id,
        prompt,
        format!("Shepherd sync from {source_label}"),
    )
    .await
}

// ── Graph Operations ──

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
            "error": msg,
            "hint": graph_error_hint(&msg),
        }));
    }

    let mut results = Vec::with_capacity(statement_count);
    for index in 0..statement_count {
        let rows: Vec<Value> = response.take(index).unwrap_or_default();
        results.push(json!({
            "index": index,
            "row_count": rows.len(),
            "record_ids": extract_record_ids(&rows),
            "graph_preview": graph_preview(&rows),
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
    let db = match librarian_db(project_id).await {
        Ok(db) => db,
        Err(error) => {
            return ToolResult::err(json!({
                "error": format!("failed to get librarian db: {error}")
            }));
        }
    };

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

pub async fn patch_canvas_document(project_id: i64, args: &Value) -> ToolResult {
    let Some(patch) = args.get("patch").and_then(|value| value.as_str()) else {
        return ToolResult::err(json!({ "error": "patch is required" }));
    };

    let current_document = match documents::get_canvas_document(project_id).await {
        Ok(Some(document)) => document,
        Ok(None) => {
            return ToolResult::err(json!({
                "error": "document:canvas does not exist"
            }));
        }
        Err(error) => {
            return ToolResult::err(json!({
                "error": format!("failed to load canvas document: {error}")
            }));
        }
    };

    let patched = match apply_text_patch(&current_document.html, patch) {
        Ok(outcome) => outcome,
        Err(error) => return ToolResult::err(json!({ "error": error })),
    };

    if let Err(message) = validate_canvas_markup_contract(&patched.new_text) {
        return ToolResult::err(json!({ "error": message }));
    }

    let patched_html = patched.new_text.clone();
    let result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(documents::upsert_canvas_document(
            project_id,
            &patched_html,
            Some("librarian"),
        ))
    });

    match result {
        Ok(document) => edit_result_with(
            "Patched canvas document",
            [
                ("project_id".to_string(), json!(project_id)),
                ("node_id".to_string(), json!(document.node_id)),
                ("added".to_string(), json!(patched.added_lines)),
                ("removed".to_string(), json!(patched.removed_lines)),
                ("updated_at".to_string(), json!(document.updated_at)),
            ]
            .into_iter()
            .collect(),
        ),
        Err(errors) => ToolResult::err(json!({
            "error": format_canvas_validation_errors(&errors),
            "details": errors,
        })),
    }
}

fn validate_canvas_markup_contract(html: &str) -> Result<(), String> {
    let fragment = scraper::Html::parse_fragment(html);
    for tag_name in CANVAS_TAGS_REQUIRING_NODE_ATTR {
        let selector = scraper::Selector::parse(tag_name)
            .map_err(|error| format!("invalid selector for {tag_name}: {error}"))?;
        for node in fragment.select(&selector) {
            let attrs = node.value();
            let node_attr = attrs.attr("node").map(str::trim).unwrap_or("");
            if node_attr.is_empty() {
                return Err(format!("<{tag_name}> requires node=\"kind:id\""));
            }
            let Some((kind, node_id)) = node_attr.split_once(':') else {
                return Err(format!("<{tag_name}> node attribute must be kind:id"));
            };
            if kind.trim().is_empty() || node_id.trim().is_empty() {
                return Err(format!("<{tag_name}> node attribute must be kind:id"));
            }
            if attrs.attr("kind").is_some() || attrs.attr("id").is_some() {
                return Err(format!(
                    "<{tag_name}> must not include separate kind/id attributes"
                ));
            }
            if *tag_name == "hirsel-doc-link" && attrs.attr("path").is_some() {
                return Err(
                    "<hirsel-doc-link> must not use path=; use node=\"kind:id\"".to_string()
                );
            }
        }
    }
    Ok(())
}

fn format_canvas_validation_errors(errors: &[DocumentValidationError]) -> String {
    errors
        .iter()
        .map(|error| error.message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn graph_node_record_key(project_id: i64, kind: &str, node_id: &str) -> Value {
    json!([project_id, kind, node_id])
}

fn is_allowed_graph_text_field(field: &str) -> bool {
    GRAPH_TEXT_FIELDS.contains(&field)
}

fn query_might_mutate_graph(query: &str) -> bool {
    let upper = query.to_uppercase();
    GRAPH_MUTATION_KEYWORDS
        .iter()
        .any(|keyword| upper.contains(keyword))
}

fn format_graph_errors(errors: std::collections::HashMap<usize, surrealdb::Error>) -> String {
    errors
        .into_iter()
        .map(|(index, error)| format!("statement {index}: {error}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn graph_error_hint(message: &str) -> Option<&'static str> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("already exists") {
        Some("Use UPSERT when creating a node that may already exist.")
    } else if lower.contains("not enough permissions") || lower.contains("iam error") {
        Some("This query tried to access or mutate graph records outside the Librarian's allowed graph scope.")
    } else if lower.contains("canvas") {
        Some("Use patch_canvas_document(patch) for canvas changes.")
    } else {
        None
    }
}

fn extract_record_ids(rows: &[Value]) -> Vec<Value> {
    rows.iter()
        .filter_map(|row| row.get("id").cloned())
        .collect()
}

fn graph_preview(rows: &[Value]) -> Vec<Value> {
    rows.iter().filter_map(graph_preview_row).take(10).collect()
}

fn graph_preview_row(row: &Value) -> Option<Value> {
    let object = row.as_object()?;
    if let (Some(kind), Some(node_id)) = (
        object.get("kind").and_then(Value::as_str),
        object.get("node_id").and_then(Value::as_str),
    ) {
        let mut preview = Map::new();
        preview.insert("type".to_string(), json!("kg_node"));
        preview.insert("kind".to_string(), json!(kind));
        preview.insert("node_id".to_string(), json!(node_id));
        if let Some(label) = object.get("label").and_then(Value::as_str) {
            preview.insert("label".to_string(), json!(label));
        }
        if let Some(id) = object.get("id") {
            preview.insert("id".to_string(), id.clone());
        }
        return Some(Value::Object(preview));
    }

    if let (Some(relation), Some(in_record), Some(out_record)) = (
        object.get("relation").and_then(Value::as_str),
        object.get("in").or_else(|| object.get("in_record")),
        object.get("out"),
    ) {
        return Some(json!({
            "type": "kg_edge",
            "relation": relation,
            "in": in_record,
            "out": out_record,
            "id": object.get("id").cloned(),
        }));
    }

    None
}
