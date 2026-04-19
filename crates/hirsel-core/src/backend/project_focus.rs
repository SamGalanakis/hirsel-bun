//! Recently-focused canvas/graph nodes per project.
//!
//! Populated when the user opens a focus overlay in the canvas. Consumed by
//! the shepherd's dynamic prompt contributor so the assistant always knows
//! what the user is looking at.

use serde::Deserialize;
use surrealdb::types::SurrealValue;

use super::db::global_db;
use super::knowledge_graph::surreal_datetime_value_to_string;
use super::runtime_settings::{keys, Defaults, RuntimeSettings};

#[derive(Debug, Clone, Deserialize, SurrealValue)]
struct RecentFocusRow {
    node_kind: String,
    node_id: String,
    focused_at: surrealdb::types::Value,
}

#[derive(Debug, Clone, Deserialize, SurrealValue)]
struct KgNodeLabelRow {
    kind: String,
    node_id: String,
    label: String,
}

#[derive(Debug, Clone)]
pub struct RecentFocusEntry {
    pub kind: String,
    pub node_id: String,
    pub label: Option<String>,
    pub focused_at: String,
}

pub async fn record_focus(project_id: i64, kind: &str, node_id: &str) -> Result<(), String> {
    let kind = kind.trim();
    let node_id = node_id.trim();
    if kind.is_empty() || node_id.is_empty() {
        return Ok(());
    }
    let db = global_db().await;
    db.query(
        "UPSERT type::record('project_recent_focus', [$project_id, $kind, $node_id]) \
         CONTENT { project_id: $project_id, node_kind: $kind, node_id: $node_id, focused_at: time::now() }",
    )
    .bind(("project_id", project_id))
    .bind(("kind", kind.to_string()))
    .bind(("node_id", node_id.to_string()))
    .await
    .map_err(|error| format!("record_focus failed: {error}"))?;
    Ok(())
}

pub async fn recent_focus(project_id: i64) -> Result<Vec<RecentFocusEntry>, String> {
    recent_focus_with_limit(project_id, None).await
}

/// Fetch the top-N recently focused nodes. If `limit_override` is `None`,
/// the effective limit is the value stored under
/// `runtime_setting:project_recent_focus.limit` (falling back to
/// [`Defaults::PROJECT_RECENT_FOCUS_LIMIT`]).
pub async fn recent_focus_with_limit(
    project_id: i64,
    limit_override: Option<usize>,
) -> Result<Vec<RecentFocusEntry>, String> {
    let limit = RuntimeSettings::resolve(
        keys::PROJECT_RECENT_FOCUS_LIMIT,
        limit_override,
        Defaults::PROJECT_RECENT_FOCUS_LIMIT,
    )
    .await;
    if limit == 0 {
        return Ok(Vec::new());
    }
    let db = global_db().await;

    let mut focus_result = db
        .query(
            "SELECT node_kind, node_id, focused_at FROM project_recent_focus \
             WHERE project_id = $project_id ORDER BY focused_at DESC LIMIT $limit",
        )
        .bind(("project_id", project_id))
        .bind(("limit", limit as i64))
        .await
        .map_err(|error| format!("recent_focus query failed: {error}"))?;
    let rows: Vec<RecentFocusRow> = focus_result
        .take(0)
        .map_err(|error| format!("recent_focus deserialize failed: {error}"))?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let pairs: Vec<Vec<String>> = rows
        .iter()
        .map(|row| vec![row.node_kind.clone(), row.node_id.clone()])
        .collect();
    let mut label_result = db
        .query(
            "SELECT kind, node_id, label FROM kg_node \
             WHERE id[0] = $project_id AND [kind, node_id] IN $pairs",
        )
        .bind(("project_id", project_id))
        .bind(("pairs", pairs))
        .await
        .map_err(|error| format!("recent_focus label query failed: {error}"))?;
    let labels: Vec<KgNodeLabelRow> = label_result.take(0).unwrap_or_default();

    let lookup = |kind: &str, node_id: &str| -> Option<String> {
        labels
            .iter()
            .find(|row| row.kind == kind && row.node_id == node_id)
            .map(|row| row.label.clone())
    };

    Ok(rows
        .into_iter()
        .map(|row| RecentFocusEntry {
            label: lookup(&row.node_kind, &row.node_id),
            focused_at: surreal_datetime_value_to_string(Some(row.focused_at)),
            kind: row.node_kind,
            node_id: row.node_id,
        })
        .collect())
}
