//! Librarian — knowledge graph agent backed by SurrealDB.
//!
//! The Librarian only gets project-scoped access to the knowledge graph tables:
//! - `kg_node` for entities such as artifacts, features, issues, decisions, and documents
//! - `kg_edge` for graph relations via `RELATE`
//! - `librarian_event` for the inbound event queue

use std::sync::OnceLock;

use chrono::{DateTime, Duration, Utc};
use lash::ToolResult;
use regex::Regex;
use serde_json::{json, Map, Value};

use crate::backend::db::global_db;
use crate::backend::live_updates::{self, LiveUpdateKind};
use crate::backend::text_patch::{apply_text_patch, is_safe_patch_field_name};
use crate::backend::tool_results::edit_result_with;

const RESERVED_GRAPH_PARAM_NAMES: &[&str] = &["project_id"];
const GRAPH_MUTATION_KEYWORDS: &[&str] = &["CREATE", "UPSERT", "UPDATE", "DELETE", "RELATE"];
const FORBIDDEN_GRAPH_KEYWORDS: &[&str] = &[
    "ACCESS", "ALTER", "ANALYZER", "BUCKET", "DEFINE", "INFO", "INSERT", "KILL", "LIVE", "OPTION",
    "REBUILD", "REMOVE", "USE",
];
const FORBIDDEN_GRAPH_TABLES: &[&str] = &[
    "counter",
    "project",
    "project_retained_context",
    "project_runtime_preparation",
    "shepherd_chat_message",
    "shepherd_live_turn",
    "shepherd_scope_state",
    "shepherd_thread",
    "shepherd_session",
    "credential",
    "librarian_event",
];
const GRAPH_TEXT_FIELDS: &[&str] = &[
    "summary",
    "description",
    "detail",
    "notes",
    "rationale",
    "markdown",
];

pub(crate) const LIBRARIAN_SURREALQL_GUIDE: &str = r#"## Knowledge Graph Querying

Use `graph_surql(query, params?)` for graph reads and writes. The backend always binds `$project_id` for you and rejects schema commands or non-graph tables.

Allowed tables:
- `kg_node`
- `kg_edge`

Canonical node record IDs:
- `type::record('kg_node', [$project_id, 'artifact', 'src/auth.rs'])`
- `type::record('kg_node', [$project_id, 'feature', 'auth'])`
- `type::record('kg_node', [$project_id, 'document', 'canvas'])`

Keep node shape stable:
- `project_id`, `kind`, `node_id`, `label`, `summary`, `source`, `metadata`, `updated_at`

Keep edge shape stable:
- `project_id`, `relation`, `metadata`, `created_at`

Preferred patterns:
- Lookup/list: `SELECT * FROM kg_node WHERE project_id = $project_id AND ...`
- Upsert node: `UPSERT type::record('kg_node', [$project_id, $kind, $node_id]) MERGE { ... }`
- Relate nodes: `RELATE $from->kg_edge->$to SET project_id = $project_id, relation = 'part_of', metadata = {}`
- Multi-step updates: `BEGIN TRANSACTION; ... COMMIT TRANSACTION;`
- Abort a bad transaction: `THROW 'reason'`

For long text refinement on an existing node, prefer `edit_graph_node_text(kind, id, field, patch)` instead of rewriting the whole node.
Editable text fields:
- `summary`
- `description`
- `detail`
- `notes`
- `rationale`
- `markdown`"#;

// ── Event Queue ──

pub async fn enqueue_event(project_id: i64, event: Value) -> Result<String, String> {
    let db = global_db().await;

    let kind = event
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let summary = event.get("summary").and_then(|v| v.as_str()).unwrap_or("");
    let files = event.get("files").cloned().unwrap_or(json!([]));
    let timestamp = event
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut result = db
        .query(
            "CREATE librarian_event SET
                project_id = $project_id,
                kind = $kind,
                summary = $summary,
                files = $files,
                timestamp = $timestamp,
                processed = false,
                created_at = time::now()",
        )
        .bind(("project_id", project_id))
        .bind(("kind", kind.to_string()))
        .bind(("summary", summary.to_string()))
        .bind(("files", files))
        .bind(("timestamp", timestamp.to_string()))
        .await
        .map_err(|e| format!("failed to enqueue event: {e}"))?;

    let created: Option<Value> = result
        .take(0)
        .map_err(|e| format!("failed to read created event: {e}"))?;

    let id = created
        .and_then(|v| v.get("id").cloned())
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    Ok(id)
}

/// Drain all pending events for a project and format them as a message
/// for the Librarian's conversation.
pub async fn drain_pending_events(project_id: i64) -> Result<Option<String>, String> {
    let db = global_db().await;

    let mut result = db
        .query(
            "SELECT * FROM librarian_event
             WHERE project_id = $project_id AND processed = false
             ORDER BY created_at ASC
             LIMIT 50",
        )
        .bind(("project_id", project_id))
        .await
        .map_err(|e| format!("failed to query events: {e}"))?;

    let events: Vec<Value> = result
        .take(0)
        .map_err(|e| format!("failed to read events: {e}"))?;

    if events.is_empty() {
        return Ok(None);
    }

    db.query(
        "UPDATE librarian_event
         SET processed = true
         WHERE project_id = $project_id AND processed = false",
    )
    .bind(("project_id", project_id))
    .await
    .map_err(|e| format!("failed to mark events processed: {e}"))?;

    let mut message = format!(
        "Process these {} knowledge event(s). Read relevant files if needed, then update the knowledge graph with `graph_surql`. Use `edit_graph_node_text` only when you need to patch a long existing text field in place. Shared chat context for the batch appears once below.\n\n",
        events.len()
    );

    if let Some(context) = build_batch_context(project_id, &events).await? {
        message.push_str("Context:\n");
        message.push_str(&context);
        message.push_str("\n\n");
    }

    for event in &events {
        let kind = event
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let summary = event.get("summary").and_then(|v| v.as_str()).unwrap_or("");
        let files = event
            .get("files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();

        message.push_str(&format!("---\nKind: {kind}\nSummary: {summary}\n"));
        if !files.is_empty() {
            message.push_str(&format!("Files: {files}\n"));
        }
        message.push('\n');
    }

    Ok(Some(message))
}

async fn build_batch_context(project_id: i64, events: &[Value]) -> Result<Option<String>, String> {
    let Some(latest) = latest_event_timestamp(events) else {
        return Ok(None);
    };
    let window_end = latest + Duration::seconds(90);
    let history = crate::backend::shepherd_runtime::get_shepherd_history(
        crate::backend::shepherd_runtime::ShepherdScope::Shepherd {
            project_id,
            workspace_path: None,
            focus: None,
        },
        24,
    )
    .await?;

    let mut out = String::new();
    for msg in history {
        let Ok(ts) = DateTime::parse_from_rfc3339(&msg.timestamp) else {
            continue;
        };
        let ts = ts.with_timezone(&Utc);
        if ts > window_end {
            continue;
        }
        append_message_context(&mut out, &msg.role, &msg.chunks_json);
    }

    if out.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}

fn latest_event_timestamp(events: &[Value]) -> Option<DateTime<Utc>> {
    events
        .iter()
        .filter_map(|event| event.get("timestamp").and_then(|v| v.as_str()))
        .filter_map(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
        .max()
}

fn append_message_context(out: &mut String, role: &str, chunks_json: &str) {
    let Ok(chunks) = serde_json::from_str::<Vec<Value>>(chunks_json) else {
        return;
    };
    for chunk in &chunks {
        if chunk.get("type").and_then(|v| v.as_str()) != Some("text") {
            continue;
        }
        let Some(content) = chunk.get("content").and_then(|v| v.as_str()) else {
            continue;
        };
        let truncated = if content.len() > 300 {
            format!("{}...", &content[..300])
        } else {
            content.to_string()
        };
        out.push_str(&format!("  [{role}]: {truncated}\n"));
    }
}

pub async fn pending_event_count(project_id: i64) -> usize {
    let db = global_db().await;
    let mut result = match db
        .query(
            "SELECT count() as c FROM librarian_event
             WHERE project_id = $project_id AND processed = false
             GROUP ALL",
        )
        .bind(("project_id", project_id))
        .await
    {
        Ok(r) => r,
        Err(_) => return 0,
    };
    let row: Option<Value> = result.take(0).unwrap_or(None);
    row.and_then(|v| v.get("c").and_then(|c| c.as_u64()))
        .unwrap_or(0) as usize
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

    if let Err(error) = validate_graph_surql(query, &params) {
        return ToolResult::err(json!({ "error": error }));
    }

    let db = global_db().await;
    let mut query_builder = db.query(query).bind(("project_id", project_id));
    if !params.is_empty() {
        query_builder = query_builder.bind(Value::Object(params));
    }

    let mut response = match query_builder.await {
        Ok(response) => response,
        Err(error) => {
            return ToolResult::err(json!({
                "error": format!("graph_surql failed: {error}")
            }));
        }
    };

    let statement_count = response.num_statements();
    let errors = response.take_errors();
    if !errors.is_empty() {
        return ToolResult::err(json!({
            "error": format_graph_errors(errors)
        }));
    }

    let mut results = Vec::with_capacity(statement_count);
    for index in 0..statement_count {
        let value: Option<Value> = response.take(index).unwrap_or(None);
        results.push(json!({
            "index": index,
            "value": value.unwrap_or(Value::Null),
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
    let db = global_db().await;

    let mut response = match db
        .query(
            "LET $node = type::record('kg_node', $node_key);
             SELECT * FROM $node WHERE project_id = $project_id;",
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

    let record: Option<Value> = match response.take(1) {
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

    let current = match record.get(field) {
        None | Some(Value::Null) => "",
        Some(Value::String(value)) => value.as_str(),
        Some(_) => {
            return ToolResult::err(json!({
                "error": format!("field '{field}' is not a text field on this node")
            }));
        }
    };

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

fn graph_node_record_key(project_id: i64, kind: &str, node_id: &str) -> Value {
    json!([project_id, kind, node_id])
}

fn is_allowed_graph_text_field(field: &str) -> bool {
    GRAPH_TEXT_FIELDS.contains(&field)
}

fn query_might_mutate_graph(query: &str) -> bool {
    GRAPH_MUTATION_KEYWORDS
        .iter()
        .any(|keyword| contains_keyword(query, keyword))
}

fn validate_graph_surql(query: &str, params: &Map<String, Value>) -> Result<(), String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err("query cannot be empty".to_string());
    }

    for key in params.keys() {
        if RESERVED_GRAPH_PARAM_NAMES.contains(&key.as_str()) {
            return Err(format!("params may not override reserved binding '${key}'"));
        }
        if !is_safe_patch_field_name(key) {
            return Err(format!("invalid parameter name: {key}"));
        }
    }

    for keyword in FORBIDDEN_GRAPH_KEYWORDS {
        if contains_keyword(trimmed, keyword) {
            return Err(format!("`{keyword}` is not allowed in graph_surql"));
        }
    }

    for table in FORBIDDEN_GRAPH_TABLES {
        if query_references_table(trimmed, table) {
            return Err(format!(
                "table `{table}` is not accessible from graph_surql"
            ));
        }
    }

    if !(trimmed.contains("kg_node") || trimmed.contains("kg_edge")) {
        return Err("graph_surql may only operate on kg_node / kg_edge".to_string());
    }

    if contains_pattern(trimmed, r"(?i)\b(?:CREATE|UPSERT)\s+kg_node\b") {
        return Err(
            "direct CREATE/UPSERT on kg_node is not allowed; use type::record('kg_node', [$project_id, ...])"
                .to_string(),
        );
    }

    if contains_pattern(trimmed, r"(?i)\b(?:CREATE|UPSERT)\s+kg_edge\b") {
        return Err(
            "direct CREATE/UPSERT on kg_edge is not allowed; use RELATE ...->kg_edge->..."
                .to_string(),
        );
    }

    let references_graph_tables_directly = contains_pattern(
        trimmed,
        r"(?i)\b(?:FROM|ONLY|UPDATE|DELETE)\s+(?:ONLY\s+)?kg_(?:node|edge)\b",
    );
    if references_graph_tables_directly
        && !contains_pattern(trimmed, r"(?i)\bproject_id\s*=\s*\$project_id\b")
    {
        return Err(
            "table scans and direct table mutations must include `project_id = $project_id`"
                .to_string(),
        );
    }

    if query_might_mutate_graph(trimmed) && !trimmed.contains("$project_id") {
        return Err("graph mutations must reference the bound `$project_id`".to_string());
    }

    Ok(())
}

fn format_graph_errors(errors: std::collections::HashMap<usize, surrealdb::Error>) -> String {
    errors
        .into_iter()
        .map(|(index, error)| format!("statement {index}: {error}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn contains_keyword(query: &str, keyword: &str) -> bool {
    let pattern = format!(r"(?i)\b{}\b", regex::escape(keyword));
    contains_pattern(query, &pattern)
}

fn contains_pattern(query: &str, pattern: &str) -> bool {
    static REGEX_CACHE: OnceLock<std::sync::Mutex<std::collections::HashMap<String, Regex>>> =
        OnceLock::new();
    let cache = REGEX_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let regex = {
        let mut cache = cache.lock().expect("regex cache lock");
        cache
            .entry(pattern.to_string())
            .or_insert_with(|| Regex::new(pattern).expect("valid graph regex"))
            .clone()
    };
    regex.is_match(query)
}

fn query_references_table(query: &str, table: &str) -> bool {
    let escaped = regex::escape(table);
    contains_pattern(
        query,
        &format!(
            r"(?i)\b(?:FROM|UPDATE|UPSERT|DELETE|CREATE|RELATE)\s+(?:ONLY\s+)?{}\b",
            escaped
        ),
    ) || contains_pattern(query, &format!(r#"(?i)type::record\(\s*["']{}\b"#, escaped))
        || contains_pattern(query, &format!(r"(?i)\b{}:", escaped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_scoped_graph_query() {
        let params = Map::new();
        let query = "SELECT * FROM kg_node WHERE project_id = $project_id AND kind = $kind;";
        assert!(validate_graph_surql(query, &params).is_ok());
    }

    #[test]
    fn rejects_non_graph_table_access() {
        let params = Map::new();
        let query = "SELECT * FROM project WHERE id = 1;";
        let error = validate_graph_surql(query, &params).unwrap_err();
        assert!(error.contains("project"));
    }

    #[test]
    fn rejects_unscoped_table_scan() {
        let params = Map::new();
        let query = "SELECT * FROM kg_node WHERE kind = 'module';";
        let error = validate_graph_surql(query, &params).unwrap_err();
        assert!(error.contains("project_id = $project_id"));
    }
}
