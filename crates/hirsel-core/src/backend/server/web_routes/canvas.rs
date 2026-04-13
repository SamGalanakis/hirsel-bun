use std::collections::BTreeMap;

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use crate::backend::db::global_db;
use crate::backend::knowledge_graph::KnowledgeGraphNodeRow;
use crate::backend::live_updates::{self, LiveUpdateKind};
use crate::backend::tasks::TaskStore;
use crate::backend::ShepherdThreadStore;

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
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused_task_id: Option<String>,
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
                    status: Some(task.status.clone()),
                    tags: None,
                    focused_task_id: None,
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
                    status: Some(thread.status.clone()),
                    tags: None,
                    focused_task_id: thread.focused_task_id.clone(),
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
            status: None,
            tags: kg.tags.clone(),
            focused_task_id: None,
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
            let relation = row.get("relation").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let in_kind = row.get("in_kind").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let in_id = row.get("in_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let out_kind = row.get("out_kind").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let out_id = row.get("out_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
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
    let json =
        serde_json::to_string(&layout).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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

#[allow(dead_code)]
const _LAYOUT_TABLE_NAME: &str = LAYOUT_TABLE;
