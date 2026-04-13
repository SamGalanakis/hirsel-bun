use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use crate::backend::db::{global_db, utc_now, DbClient};
use crate::backend::live_updates::{self, LiveUpdateKind};

const CANVAS_NODE_ID: &str = "canvas";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCanvasDocument {
    pub node_id: String,
    pub label: String,
    pub html: String,
    pub source: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentValidationError {
    pub code: String,
    pub message: String,
    pub tag: Option<String>,
    pub attribute: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Deserialize, SurrealValue)]
struct CanvasRow {
    html: Option<String>,
    source: Option<String>,
    #[serde(default)]
    updated_at: Option<surrealdb::types::Value>,
}

fn db() -> impl std::future::Future<Output = &'static DbClient> {
    global_db()
}

pub async fn get_canvas_document(project_id: i64) -> Result<Option<ProjectCanvasDocument>, String> {
    let db = db().await;
    let mut response = db
        .query("SELECT html, source, updated_at FROM type::record('project_canvas', $pid)")
        .bind(("pid", project_id))
        .await
        .map_err(|error| format!("failed to load canvas document: {error}"))?;
    let record: Option<CanvasRow> = response
        .take(0)
        .map_err(|error| format!("failed to decode canvas document: {error}"))?;

    Ok(record.map(|row| ProjectCanvasDocument {
        node_id: CANVAS_NODE_ID.to_string(),
        label: "Canvas".to_string(),
        html: row.html.unwrap_or_default(),
        source: row.source,
        updated_at: crate::backend::knowledge_graph::surreal_datetime_value_to_string(
            row.updated_at,
        ),
    }))
}

pub async fn upsert_canvas_document(
    project_id: i64,
    html: &str,
    source: Option<&str>,
) -> Result<ProjectCanvasDocument, Vec<DocumentValidationError>> {
    let db = db().await;
    let now = utc_now();
    let html_owned = html.to_string();
    let source_owned = source.map(ToOwned::to_owned);

    if let Err(error) = db
        .query(
            "UPSERT type::record('project_canvas', $pid) MERGE {
                 html: $html,
                 source: $source,
             }",
        )
        .bind(("pid", project_id))
        .bind(("html", html_owned))
        .bind(("source", source_owned.clone()))
        .await
    {
        return Err(vec![DocumentValidationError {
            code: "save_failed".to_string(),
            message: format!("failed to save canvas document: {error}"),
            tag: None,
            attribute: None,
            value: None,
        }]);
    }

    live_updates::publish_project(project_id, LiveUpdateKind::ProjectSurfaceChanged);

    Ok(ProjectCanvasDocument {
        node_id: CANVAS_NODE_ID.to_string(),
        label: "Canvas".to_string(),
        html: html.to_string(),
        source: source_owned,
        updated_at: now,
    })
}
