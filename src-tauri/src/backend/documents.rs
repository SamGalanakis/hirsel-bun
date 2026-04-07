use std::collections::BTreeSet;

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use surrealdb::types::Datetime;
use surrealdb::types::SurrealValue;

use crate::backend::db::{global_db, utc_now, DbClient};
use crate::backend::knowledge_graph::{
    DocumentEdgeQueueRow, KnowledgeGraphNodeRow, KnowledgeGraphTextRow,
};
use crate::backend::live_updates::{self, LiveUpdateKind};

const DOCUMENT_KIND: &str = "document";
const DOCUMENTS_EDGE: &str = "documents";
const REFERENCES_EDGE: &str = "references";
const CANVAS_NODE_ID: &str = "canvas";

const DOCUMENT_REFERENCE_TAGS: &[&str] = &[
    "hirsel-node-ref",
    "hirsel-node-field",
    "hirsel-node-list",
    "hirsel-doc-target",
    "hirsel-doc-link",
    "hirsel-doc-embed",
];

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentValidationResult {
    pub references: Vec<DocumentReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct DocumentReference {
    pub kind: String,
    pub node_id: String,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct GraphDocumentNodeRecord {
    project_id: i64,
    kind: String,
    node_id: String,
    label: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    source: Option<String>,
    updated_at: Datetime,
}

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct GraphNodeLookupRecord {
    kind: String,
    node_id: String,
    label: Option<String>,
}

fn document_record_value(project_id: i64, node_id: &str) -> Value {
    json!([project_id, DOCUMENT_KIND, node_id])
}

fn graph_record_value(project_id: i64, kind: &str, node_id: &str) -> Value {
    json!([project_id, kind, node_id])
}

fn db() -> impl std::future::Future<Output = &'static DbClient> {
    global_db()
}

fn parse_node_reference(value: &str) -> Option<(String, String)> {
    let trimmed = value.trim();
    let (kind, node_id) = trimmed.split_once(':')?;
    let kind = kind.trim();
    let node_id = node_id.trim();
    if kind.is_empty() || node_id.is_empty() {
        return None;
    }
    Some((kind.to_string(), node_id.to_string()))
}

async fn resolve_graph_node(
    project_id: i64,
    kind: &str,
    node_id: &str,
) -> Result<Option<GraphNodeLookupRecord>, String> {
    let db = db().await;
    let node_key = graph_record_value(project_id, kind, node_id);
    let mut response = db
        .query(
            "LET $node = type::record('kg_node', $node_key);
             SELECT kind, node_id, label FROM $node WHERE project_id = $project_id;",
        )
        .bind(("project_id", project_id))
        .bind(("node_key", node_key))
        .await
        .map_err(|error| format!("failed to resolve graph node: {error}"))?;
    response
        .take(1)
        .map_err(|error| format!("failed to decode graph node: {error}"))
}

pub async fn get_canvas_document(project_id: i64) -> Result<Option<ProjectCanvasDocument>, String> {
    let db = db().await;
    let node_key = document_record_value(project_id, CANVAS_NODE_ID);
    let mut response = db
        .query(
            "LET $node = type::record('kg_node', $node_key);
             SELECT * FROM $node WHERE project_id = $project_id;",
        )
        .bind(("project_id", project_id))
        .bind(("node_key", node_key))
        .await
        .map_err(|error| format!("failed to load canvas document: {error}"))?;
    let record: Option<KnowledgeGraphNodeRow> = response
        .take(1)
        .map_err(|error| format!("failed to decode canvas document: {error}"))?;

    Ok(record.map(|record| ProjectCanvasDocument {
        node_id: record.node_id,
        label: record.label,
        html: record.content.unwrap_or_default(),
        source: record.source,
        updated_at: record.updated_at.to_string(),
    }))
}

pub async fn validate_document_html(
    project_id: i64,
    html: &str,
) -> Result<DocumentValidationResult, Vec<DocumentValidationError>> {
    let fragment = Html::parse_fragment(html);
    let mut errors = Vec::new();
    let mut refs = BTreeSet::new();

    for tag_name in DOCUMENT_REFERENCE_TAGS {
        let selector = Selector::parse(tag_name).expect("valid document reference selector");
        for node in fragment.select(&selector) {
            let attrs = node.value();
            let relation = if *tag_name == "hirsel-doc-target" {
                DOCUMENTS_EDGE
            } else {
                REFERENCES_EDGE
            };
            let raw = attrs.attr("node").map(str::trim).unwrap_or("");
            if raw.is_empty() {
                errors.push(DocumentValidationError {
                    code: "missing_node".to_string(),
                    message: format!("<{tag_name}> requires a non-empty 'node' attribute"),
                    tag: Some(tag_name.to_string()),
                    attribute: Some("node".to_string()),
                    value: None,
                });
                continue;
            }

            let Some((kind, node_id)) = parse_node_reference(raw) else {
                errors.push(DocumentValidationError {
                    code: "invalid_node_reference".to_string(),
                    message: format!(
                        "<{tag_name}> has invalid node reference '{raw}'. Expected 'kind:id'."
                    ),
                    tag: Some(tag_name.to_string()),
                    attribute: Some("node".to_string()),
                    value: Some(raw.to_string()),
                });
                continue;
            };

            match resolve_graph_node(project_id, &kind, &node_id).await {
                Ok(Some(_)) => {
                    refs.insert(DocumentReference {
                        kind,
                        node_id,
                        relation: relation.to_string(),
                    });
                }
                Ok(None) => {
                    errors.push(DocumentValidationError {
                        code: "unknown_node_reference".to_string(),
                        message: format!("<{tag_name}> references unknown node '{raw}'"),
                        tag: Some(tag_name.to_string()),
                        attribute: Some("node".to_string()),
                        value: Some(raw.to_string()),
                    });
                }
                Err(error) => {
                    errors.push(DocumentValidationError {
                        code: "reference_lookup_failed".to_string(),
                        message: error,
                        tag: Some(tag_name.to_string()),
                        attribute: Some("node".to_string()),
                        value: Some(raw.to_string()),
                    });
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(DocumentValidationResult {
            references: refs.into_iter().collect(),
        })
    } else {
        Err(errors)
    }
}

pub async fn upsert_canvas_document(
    project_id: i64,
    html: &str,
    source: Option<&str>,
) -> Result<ProjectCanvasDocument, Vec<DocumentValidationError>> {
    let validation = validate_document_html(project_id, html).await?;
    let db = db().await;
    let now = utc_now();

    let record = GraphDocumentNodeRecord {
        project_id,
        kind: DOCUMENT_KIND.to_string(),
        node_id: CANVAS_NODE_ID.to_string(),
        label: "Canvas".to_string(),
        content: Some(html.to_string()),
        source: source.map(ToOwned::to_owned),
        updated_at: Datetime::from(now.parse::<chrono::DateTime<chrono::Utc>>().map_err(
            |error| {
                vec![DocumentValidationError {
                    code: "save_failed".to_string(),
                    message: format!("failed to parse canvas timestamp: {error}"),
                    tag: None,
                    attribute: None,
                    value: None,
                }]
            },
        )?),
    };

    if let Err(error) = db
        .query(
            "UPSERT type::record('kg_node', $node_key)
             CONTENT $record;",
        )
        .bind((
            "node_key",
            document_record_value(project_id, CANVAS_NODE_ID),
        ))
        .bind(("record", json!(record)))
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

    if let Err(error) =
        replace_document_reference_edges(project_id, CANVAS_NODE_ID, &validation.references).await
    {
        return Err(vec![DocumentValidationError {
            code: "reference_sync_failed".to_string(),
            message: error,
            tag: None,
            attribute: None,
            value: None,
        }]);
    }

    live_updates::publish_project(project_id, LiveUpdateKind::ProjectSurfaceChanged);
    live_updates::publish_project(project_id, LiveUpdateKind::KnowledgeGraphChanged);

    Ok(ProjectCanvasDocument {
        node_id: CANVAS_NODE_ID.to_string(),
        label: "Canvas".to_string(),
        html: html.to_string(),
        source: source.map(ToOwned::to_owned),
        updated_at: now,
    })
}

async fn replace_document_reference_edges(
    project_id: i64,
    document_node_id: &str,
    refs: &[DocumentReference],
) -> Result<(), String> {
    let db = db().await;
    let from = graph_record_value(project_id, DOCUMENT_KIND, document_node_id);

    db.query(
        "DELETE kg_edge WHERE project_id = $project_id AND out = $from AND relation IN ['references', 'documents'];",
    )
    .bind(("project_id", project_id))
    .bind(("from", from.clone()))
    .await
    .map_err(|error| format!("failed to clear document reference edges: {error}"))?;

    for reference in refs {
        let to = graph_record_value(project_id, &reference.kind, &reference.node_id);
        db.query(
            "RELATE $from->kg_edge->$to SET project_id = $project_id, relation = $relation, metadata = {}, created_at = time::now();",
        )
        .bind(("project_id", project_id))
        .bind(("from", from.clone()))
        .bind(("to", to))
        .bind(("relation", reference.relation.clone()))
        .await
        .map_err(|error| format!("failed to write document reference edge: {error}"))?;
    }

    Ok(())
}

/// Process the kg_doc_edge_queue: for each queued document node, re-derive
/// reference edges from its HTML content, then delete the queue entry.
pub async fn process_doc_edge_queue() -> Result<usize, String> {
    let db = db().await;
    let mut result = db
        .query("SELECT * FROM kg_doc_edge_queue LIMIT 50")
        .await
        .map_err(|e| format!("failed to read doc edge queue: {e}"))?;
    let rows: Vec<DocumentEdgeQueueRow> = result.take(0).unwrap_or_default();
    if rows.is_empty() {
        return Ok(0);
    }

    let mut processed = 0;
    for row in rows {
        if row.project_id == 0 || row.node_id.is_empty() {
            continue;
        }

        // Fetch the document's content
        let mut content_result = db
            .query("SELECT content FROM kg_node WHERE project_id = $pid AND kind = 'document' AND node_id = $nid LIMIT 1")
            .bind(("pid", row.project_id))
            .bind(("nid", row.node_id.clone()))
            .await
            .map_err(|e| format!("failed to fetch doc content: {e}"))?;
        let content_rows: Vec<KnowledgeGraphTextRow> = content_result.take(0).unwrap_or_default();
        let content = content_rows
            .first()
            .and_then(|row| row.content.as_deref())
            .unwrap_or("");

        // Parse references from HTML (synchronous — scraper is !Send)
        let refs = extract_document_references(content);
        let _ = replace_document_reference_edges(row.project_id, &row.node_id, &refs).await;

        // Delete the queue entry
        let _ = db.query("DELETE $id").bind(("id", row.id)).await;
        processed += 1;
    }
    Ok(processed)
}

fn extract_document_references(html: &str) -> Vec<DocumentReference> {
    if html.is_empty() {
        return Vec::new();
    }
    let fragment = Html::parse_fragment(html);
    let mut refs = Vec::new();
    for tag_name in DOCUMENT_REFERENCE_TAGS {
        let Ok(selector) = Selector::parse(tag_name) else {
            continue;
        };
        for node in fragment.select(&selector) {
            let relation = if *tag_name == "hirsel-doc-target" {
                DOCUMENTS_EDGE
            } else {
                REFERENCES_EDGE
            };
            let raw = node.value().attr("node").map(str::trim).unwrap_or("");
            if let Some((kind, ref_id)) = parse_node_reference(raw) {
                refs.push(DocumentReference {
                    kind,
                    node_id: ref_id,
                    relation: relation.to_string(),
                });
            }
        }
    }
    refs
}

/// Start a background task that polls the doc edge queue and processes entries.
pub fn spawn_doc_edge_worker() {
    tokio::spawn(async {
        loop {
            match process_doc_edge_queue().await {
                Ok(n) if n > 0 => {
                    tracing::debug!(processed = n, "processed doc edge queue entries");
                    // If we processed items, check again soon in case more arrived
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                Ok(_) => {
                    // Queue empty — sleep longer
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "doc edge queue processing failed");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_node_reference() {
        assert_eq!(
            parse_node_reference("feature:auth"),
            Some(("feature".to_string(), "auth".to_string()))
        );
        assert_eq!(parse_node_reference("feature"), None);
    }
}
