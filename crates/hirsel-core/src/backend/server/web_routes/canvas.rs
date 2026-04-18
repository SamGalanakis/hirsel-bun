use std::collections::BTreeMap;

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use crate::backend::companion_actions;
use crate::backend::db::global_db;
use crate::backend::knowledge_graph::KnowledgeGraphNodeRow;
use crate::backend::librarian_events::record_user_activity;
use crate::backend::live_updates::{self, LiveUpdateKind};
use crate::backend::tasks::TaskStore;
use crate::backend::ShepherdThreadStore;

const USER_KIND_KG: &[&str] = &["document", "goal", "decision"];
const EDITABLE_KG: &[&str] = &[
    "document",
    "goal",
    "decision",
    "component",
    "entity",
    "convention",
    "fact",
];

const LAYOUT_TABLE: &str = "project_canvas_layout";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasNode {
    pub kind: String,
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasEdge {
    pub from: String,
    pub to: String,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CanvasView {
    pub nodes: Vec<CanvasNode>,
    pub edges: Vec<CanvasEdge>,
    pub layout: BTreeMap<String, CanvasPosition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct CanvasLayoutRecord {
    project_id: i64,
    layout_json: Option<String>,
}

pub async fn get_canvas(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let db = global_db().await;
    let mut nodes: Vec<CanvasNode> = Vec::new();

    // Tasks
    if let Ok(store) = TaskStore::open().await {
        if let Ok(tasks) = store.list_project_tasks(project_id).await {
            for task in tasks {
                nodes.push(CanvasNode {
                    kind: "task".to_string(),
                    id: task.id.clone(),
                    label: task.title.clone(),
                    content: task.content.clone(),
                    subtype: None,
                    status: Some(task.status.clone()),
                    tags: None,
                    focused_task_id: None,
                    highlight: None,
                    updated_at: task.updated_at.clone(),
                });
            }
        }
    }

    // Threads
    if let Ok(store) = ShepherdThreadStore::open().await {
        if let Ok(threads) = store.list_project_threads(project_id).await {
            for thread in threads {
                if thread.archived_at.is_some() {
                    continue;
                }
                nodes.push(CanvasNode {
                    kind: "thread".to_string(),
                    id: thread.id.clone(),
                    label: thread.title.clone(),
                    content: Some(thread.summary.clone()),
                    subtype: None,
                    status: Some(thread.status.clone()),
                    tags: None,
                    focused_task_id: thread.focused_task_id.clone(),
                    highlight: thread.highlight.clone(),
                    updated_at: thread.updated_at.clone(),
                });
            }
        }
    }

    // Knowledge graph nodes
    let mut kg_response = db
        .query("SELECT * FROM kg_node WHERE id[0] = $project_id ORDER BY updated_at DESC LIMIT 500")
        .bind(("project_id", project_id))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("kg query: {e}")))?;
    let kg_nodes: Vec<KnowledgeGraphNodeRow> = kg_response.take(0).unwrap_or_default();

    for kg in kg_nodes {
        nodes.push(CanvasNode {
            kind: kg.kind.clone(),
            id: kg.node_id.clone(),
            label: if kg.label.is_empty() {
                kg.node_id.clone()
            } else {
                kg.label.clone()
            },
            content: kg.content.clone(),
            subtype: kg.subtype.clone(),
            status: None,
            tags: kg.tags.clone(),
            focused_task_id: None,
            highlight: None,
            updated_at: crate::backend::knowledge_graph::surreal_datetime_value_to_string(
                kg.updated_at.clone(),
            ),
        });
    }

    // Edges from kg_edge
    let mut edges: Vec<CanvasEdge> = Vec::new();
    let mut edge_response = db
        .query(
            "SELECT relation, `in`[1] AS in_kind, `in`[2] AS in_id, out[1] AS out_kind, out[2] AS out_id \
             FROM kg_edge WHERE `in`[0] = $project_id AND out[0] = $project_id LIMIT 1000",
        )
        .bind(("project_id", project_id))
        .await;
    if let Ok(ref mut r) = edge_response {
        let rows: Vec<serde_json::Value> = r.take(0).unwrap_or_default();
        for row in rows {
            let relation = row
                .get("relation")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let in_kind = row
                .get("in_kind")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let in_id = row
                .get("in_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let out_kind = row
                .get("out_kind")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let out_id = row
                .get("out_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !in_kind.is_empty() && !out_kind.is_empty() {
                edges.push(CanvasEdge {
                    from: format!("{in_kind}:{in_id}"),
                    to: format!("{out_kind}:{out_id}"),
                    relation,
                });
            }
        }
    }
    // Auto edge: thread → focused task
    for node in &nodes {
        if node.kind == "thread" {
            if let Some(task_id) = &node.focused_task_id {
                edges.push(CanvasEdge {
                    from: format!("thread:{}", node.id),
                    to: format!("task:{}", task_id),
                    relation: "focused_by".to_string(),
                });
            }
        }
    }

    // Layout
    let layout = load_layout(project_id).await;

    Ok(Json(CanvasView {
        nodes,
        edges,
        layout,
    }))
}

async fn load_layout(project_id: i64) -> BTreeMap<String, CanvasPosition> {
    let db = global_db().await;
    let record: Option<CanvasLayoutRecord> = db
        .query("SELECT * FROM type::record('project_canvas_layout', $pid)")
        .bind(("pid", project_id))
        .await
        .ok()
        .and_then(|mut r| r.take(0).ok())
        .and_then(|v: Vec<CanvasLayoutRecord>| v.into_iter().next());

    record
        .and_then(|r| r.layout_json)
        .and_then(|json| serde_json::from_str::<BTreeMap<String, CanvasPosition>>(&json).ok())
        .unwrap_or_default()
}

#[derive(Deserialize)]
pub struct PatchLayoutBody {
    #[serde(flatten)]
    positions: BTreeMap<String, CanvasPosition>,
}

pub async fn patch_layout(
    Path(project_id): Path<i64>,
    Json(body): Json<PatchLayoutBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let db = global_db().await;
    let mut layout = load_layout(project_id).await;
    for (key, pos) in body.positions {
        layout.insert(key, pos);
    }
    let json = serde_json::to_string(&layout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let _ = db
        .query("UPSERT type::record('project_canvas_layout', $pid) MERGE { project_id: $pid, layout_json: $json }")
        .bind(("pid", project_id))
        .bind(("json", json))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    live_updates::publish_project(project_id, LiveUpdateKind::CanvasLayoutChanged);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_layout(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let db = global_db().await;
    let _ = db
        .query("DELETE type::record('project_canvas_layout', $pid)")
        .bind(("pid", project_id))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    live_updates::publish_project(project_id, LiveUpdateKind::CanvasLayoutChanged);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct CreateCanvasNodeBody {
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub content: Option<String>,
    /// For `kind = "document"`: either `"markdown"` (default) or `"html"`.
    /// Ignored for other kinds.
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

pub async fn create_canvas_node(
    Path(project_id): Path<i64>,
    Json(body): Json<CreateCanvasNodeBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let title = body.title.trim().to_string();
    if title.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "title is required".into()));
    }
    let node_id: String;
    match body.kind.as_str() {
        "task" => {
            let store = TaskStore::open().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("open tasks: {e}"),
                )
            })?;
            let task = store
                .create_task(project_id, &title, body.content.as_deref())
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("create task: {e}"),
                    )
                })?;
            node_id = task.id;
            live_updates::publish_project(project_id, LiveUpdateKind::TasksChanged);
        }
        k if USER_KIND_KG.contains(&k) => {
            node_id = slugify_with_hash(&title);
            let subtype: Option<String> = if k == "document" {
                Some(normalize_document_subtype(body.subtype.as_deref()))
            } else {
                None
            };
            let tags = normalize_tags(body.tags.clone().unwrap_or_default());
            let db = global_db().await;
            let _ = db
                .query(
                    "UPSERT type::record('kg_node', [$pid, $kind, $nid]) MERGE { \
                       project_id: $pid, kind: $kind, node_id: $nid, label: $label, \
                       content: $content, subtype: $subtype, source: 'user', tags: $tags }",
                )
                .bind(("pid", project_id))
                .bind(("kind", k.to_string()))
                .bind(("nid", node_id.clone()))
                .bind(("label", title.clone()))
                .bind(("content", body.content.clone().unwrap_or_default()))
                .bind(("subtype", subtype))
                .bind(("tags", tags))
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("create kg_node: {e}"),
                    )
                })?;
            live_updates::publish_project(project_id, LiveUpdateKind::KnowledgeGraphChanged);
        }
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("unsupported create kind: {other}"),
            ));
        }
    }
    live_updates::publish_project(project_id, LiveUpdateKind::CanvasLayoutChanged);
    record_user_activity(
        project_id,
        "user_created",
        serde_json::json!({ "kind": body.kind, "node_id": node_id, "title": title }),
    )
    .await;
    Ok(Json(
        serde_json::json!({ "ok": true, "id": node_id, "kind": body.kind }),
    ))
}

#[derive(Deserialize)]
pub struct UpdateCanvasNodeBody {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    /// For `kind = "document"`: switch rendering between `"markdown"` and `"html"`.
    #[serde(default)]
    pub subtype: Option<String>,
    /// Replace the node's tags entirely. Normalized (trim/lowercase/dedupe) server-side.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

pub async fn update_canvas_node(
    Path((project_id, kind, node_id)): Path<(i64, String, String)>,
    Json(body): Json<UpdateCanvasNodeBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match kind.as_str() {
        "task" => {
            let store = TaskStore::open().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("open tasks: {e}"),
                )
            })?;
            let content_arg: Option<Option<&str>> = body.content.as_deref().map(Some);
            store
                .update_task(
                    &node_id,
                    body.title.as_deref(),
                    body.status.as_deref(),
                    content_arg,
                )
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("update task: {e}"),
                    )
                })?;
            live_updates::publish_project(project_id, LiveUpdateKind::TasksChanged);
        }
        k if EDITABLE_KG.contains(&k) => {
            let db = global_db().await;
            // Only update fields that were provided. Build a MERGE payload dynamically.
            let mut merge = serde_json::Map::new();
            if let Some(t) = &body.title {
                merge.insert("label".into(), serde_json::Value::String(t.clone()));
            }
            if let Some(c) = &body.content {
                merge.insert("content".into(), serde_json::Value::String(c.clone()));
            }
            if let Some(s) = body.subtype.as_deref() {
                if k == "document" {
                    merge.insert(
                        "subtype".into(),
                        serde_json::Value::String(normalize_document_subtype(Some(s))),
                    );
                }
            }
            if let Some(tags) = body.tags.clone() {
                merge.insert(
                    "tags".into(),
                    serde_json::Value::Array(
                        normalize_tags(tags)
                            .into_iter()
                            .map(serde_json::Value::String)
                            .collect(),
                    ),
                );
            }
            if merge.is_empty() {
                return Ok(Json(serde_json::json!({ "ok": true })));
            }
            let merge_str = serde_json::Value::Object(merge).to_string();
            let _ = db
                .query(&format!(
                    "UPDATE type::record('kg_node', [$pid, $kind, $nid]) MERGE {merge_str}"
                ))
                .bind(("pid", project_id))
                .bind(("kind", kind.clone()))
                .bind(("nid", node_id.clone()))
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("update kg_node: {e}"),
                    )
                })?;
            live_updates::publish_project(project_id, LiveUpdateKind::KnowledgeGraphChanged);
        }
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("unsupported update kind: {other}"),
            ));
        }
    }
    record_user_activity(
        project_id,
        "user_edited",
        serde_json::json!({ "kind": kind, "node_id": node_id }),
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_canvas_node(
    Path((project_id, kind, node_id)): Path<(i64, String, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match kind.as_str() {
        "task" => {
            let store = TaskStore::open().await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("open tasks: {e}"),
                )
            })?;
            store.delete_task(&node_id).await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("delete task: {e}"),
                )
            })?;
            live_updates::publish_project(project_id, LiveUpdateKind::TasksChanged);
            live_updates::publish_project(project_id, LiveUpdateKind::CanvasLayoutChanged);
        }
        "thread" => {
            crate::backend::shepherd_runtime::archive_thread(project_id, &node_id)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("archive thread: {e}"),
                    )
                })?;
            live_updates::publish_project(project_id, LiveUpdateKind::ThreadsChanged);
            live_updates::publish_project(project_id, LiveUpdateKind::CanvasLayoutChanged);
        }
        "component" | "entity" | "decision" | "fact" | "goal" | "convention" | "document" => {
            let db = global_db().await;
            let _ = db
                .query(
                    "DELETE FROM kg_edge WHERE (`in` = type::record('kg_node', [$pid, $kind, $nid])) OR (out = type::record('kg_node', [$pid, $kind, $nid])); \
                     DELETE type::record('kg_node', [$pid, $kind, $nid])",
                )
                .bind(("pid", project_id))
                .bind(("kind", kind.clone()))
                .bind(("nid", node_id.clone()))
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("delete kg_node: {e}")))?;
            live_updates::publish_project(project_id, LiveUpdateKind::KnowledgeGraphChanged);
            live_updates::publish_project(project_id, LiveUpdateKind::CanvasLayoutChanged);
        }
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("unsupported canvas node kind: {other}"),
            ));
        }
    }
    record_user_activity(
        project_id,
        "user_deleted",
        serde_json::json!({ "kind": kind, "node_id": node_id }),
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Validate a document subtype, defaulting to `"markdown"` for missing or unknown values.
/// Only `"markdown"` and `"html"` are accepted.
fn normalize_document_subtype(raw: Option<&str>) -> String {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("html") => "html".to_string(),
        _ => "markdown".to_string(),
    }
}

/// Normalize a list of tags: trim, lowercase, drop empties, dedupe while
/// preserving first-seen order. Keeps tags short and comparable across the app.
pub(crate) fn normalize_tags(raw: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(raw.len());
    for tag in raw {
        let normalized = tag.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }
    out
}

fn slugify_with_hash(title: &str) -> String {
    let mut slug = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if ch.is_whitespace() || ch == '-' || ch == '_' {
            if !slug.ends_with('-') {
                slug.push('-');
            }
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug.push_str("node");
    }
    // short hash disambiguates collisions
    let suffix = uuid::Uuid::new_v4().to_string()[..6].to_string();
    format!("{slug}-{suffix}")
}

pub async fn drain_companion_actions(
    Path(project_id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let actions = companion_actions::drain(project_id);
    Ok(Json(serde_json::json!({ "actions": actions })))
}

#[allow(dead_code)]
const _LAYOUT_TABLE_NAME: &str = LAYOUT_TABLE;
