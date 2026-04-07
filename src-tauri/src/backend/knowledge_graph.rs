use serde::Deserialize;
#[cfg(feature = "server")]
use serde::Serialize;
#[cfg(feature = "server")]
use serde_json::{Map as JsonMap, Value as JsonValue};
use surrealdb::types::{Datetime, Object, RecordId, SurrealValue};
#[cfg(feature = "server")]
use surrealdb::types::{RecordIdKey, ToSql, Value as SurrealDataValue};

#[derive(Debug, Clone, Deserialize, SurrealValue)]
pub(crate) struct KnowledgeGraphNodeRow {
    pub id: RecordId,
    pub project_id: i64,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub node_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub metadata: Object,
    pub updated_at: Datetime,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Deserialize, SurrealValue)]
pub(crate) struct KnowledgeGraphEdgeRow {
    pub id: RecordId,
    pub project_id: i64,
    #[serde(default)]
    pub relation: String,
    #[serde(rename = "in")]
    pub in_record: RecordId,
    pub out: RecordId,
    #[serde(default)]
    pub metadata: Object,
    pub created_at: Datetime,
}

#[derive(Debug, Clone, Deserialize, SurrealValue)]
pub(crate) struct KnowledgeGraphTextRow {
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Deserialize, SurrealValue)]
pub(crate) struct DocumentEdgeQueueRow {
    pub id: RecordId,
    pub project_id: i64,
    pub node_id: String,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApiKnowledgeGraphRecordId {
    #[serde(rename = "tb")]
    pub tb: String,
    pub id: JsonValue,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApiKnowledgeGraphNode {
    pub id: ApiKnowledgeGraphRecordId,
    pub project_id: i64,
    pub kind: String,
    pub node_id: String,
    pub label: String,
    pub summary: Option<String>,
    pub content: Option<String>,
    pub confidence: Option<String>,
    pub source: Option<String>,
    pub metadata: JsonMap<String, JsonValue>,
    pub updated_at: String,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApiKnowledgeGraphEdge {
    pub id: ApiKnowledgeGraphRecordId,
    pub project_id: i64,
    pub relation: String,
    #[serde(rename = "in")]
    pub in_record: ApiKnowledgeGraphRecordId,
    pub out: ApiKnowledgeGraphRecordId,
    pub metadata: JsonMap<String, JsonValue>,
    pub created_at: String,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApiKnowledgeGraph {
    pub nodes: Vec<ApiKnowledgeGraphNode>,
    pub edges: Vec<ApiKnowledgeGraphEdge>,
}

#[cfg(feature = "server")]
impl From<KnowledgeGraphNodeRow> for ApiKnowledgeGraphNode {
    fn from(value: KnowledgeGraphNodeRow) -> Self {
        Self {
            id: record_id_to_api(value.id),
            project_id: value.project_id,
            kind: value.kind,
            node_id: value.node_id,
            label: value.label,
            summary: value.summary,
            content: value.content,
            confidence: value.confidence,
            source: value.source,
            metadata: surreal_object_to_json_map(value.metadata),
            updated_at: value.updated_at.to_string(),
        }
    }
}

#[cfg(feature = "server")]
impl From<KnowledgeGraphEdgeRow> for ApiKnowledgeGraphEdge {
    fn from(value: KnowledgeGraphEdgeRow) -> Self {
        Self {
            id: record_id_to_api(value.id),
            project_id: value.project_id,
            relation: value.relation,
            in_record: record_id_to_api(value.in_record),
            out: record_id_to_api(value.out),
            metadata: surreal_object_to_json_map(value.metadata),
            created_at: value.created_at.to_string(),
        }
    }
}

#[cfg(feature = "server")]
fn record_id_to_api(record_id: RecordId) -> ApiKnowledgeGraphRecordId {
    ApiKnowledgeGraphRecordId {
        tb: record_id.table.to_string(),
        id: record_id_key_to_json(record_id.key),
    }
}

#[cfg(feature = "server")]
fn record_id_key_to_json(value: RecordIdKey) -> JsonValue {
    match value {
        RecordIdKey::Number(number) => JsonValue::Number(number.into()),
        RecordIdKey::String(text) => JsonValue::String(text),
        RecordIdKey::Uuid(uuid) => JsonValue::String(uuid.to_string()),
        RecordIdKey::Array(items) => JsonValue::Array(
            items
                .into_iter()
                .map(surreal_value_to_json)
                .collect::<Vec<_>>(),
        ),
        RecordIdKey::Object(object) => JsonValue::Object(surreal_object_to_json_map(object)),
        RecordIdKey::Range(range) => JsonValue::String(range.to_sql()),
    }
}

#[cfg(feature = "server")]
fn surreal_object_to_json_map(value: Object) -> JsonMap<String, JsonValue> {
    value
        .into_iter()
        .map(|(key, value)| (key, surreal_value_to_json(value)))
        .collect()
}

#[cfg(feature = "server")]
fn surreal_value_to_json(value: SurrealDataValue) -> JsonValue {
    match value {
        SurrealDataValue::RecordId(record_id) => JsonValue::Object(
            [
                (
                    "tb".to_string(),
                    JsonValue::String(record_id.table.to_string()),
                ),
                ("id".to_string(), record_id_key_to_json(record_id.key)),
            ]
            .into_iter()
            .collect(),
        ),
        SurrealDataValue::Array(items) => JsonValue::Array(
            items
                .into_iter()
                .map(surreal_value_to_json)
                .collect::<Vec<_>>(),
        ),
        SurrealDataValue::Object(object) => JsonValue::Object(surreal_object_to_json_map(object)),
        SurrealDataValue::Set(items) => JsonValue::Array(
            items
                .into_iter()
                .map(surreal_value_to_json)
                .collect::<Vec<_>>(),
        ),
        other => other.into_json_value(),
    }
}
