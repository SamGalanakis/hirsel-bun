use serde::Deserialize;
use serde::Serialize;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;
use surrealdb::types::{Object, RecordId, SurrealValue};
use surrealdb::types::{RecordIdKey, ToSql, Value as SurrealDataValue};

#[derive(Debug, Clone, Deserialize, SurrealValue)]
pub(crate) struct KnowledgeGraphNodeRow {
    pub id: RecordId,
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
    pub subtype: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, surrealdb::types::Value>>,
    #[serde(default)]
    pub updated_at: Option<surrealdb::types::Value>,
    #[serde(default)]
    pub read_by_search_context: Option<surrealdb::types::Value>,
}

#[derive(Debug, Clone, Deserialize, SurrealValue)]
pub(crate) struct KnowledgeGraphEdgeRow {
    pub id: RecordId,
    #[serde(default)]
    pub relation: String,
    #[serde(rename = "in", alias = "in_record", default)]
    pub in_record: Option<RecordId>,
    #[serde(default)]
    pub out: Option<RecordId>,
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, surrealdb::types::Value>>,
    #[serde(default)]
    pub created_at: Option<surrealdb::types::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApiKnowledgeGraphRecordId {
    #[serde(rename = "tb")]
    pub tb: String,
    pub id: JsonValue,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApiKnowledgeGraphNode {
    pub id: ApiKnowledgeGraphRecordId,
    pub kind: String,
    pub node_id: String,
    pub label: String,
    pub summary: Option<String>,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source: Option<String>,
    pub metadata: JsonMap<String, JsonValue>,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_by_search_context: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApiKnowledgeGraphEdge {
    pub id: ApiKnowledgeGraphRecordId,
    pub relation: String,
    #[serde(rename = "in")]
    pub in_record: ApiKnowledgeGraphRecordId,
    pub out: ApiKnowledgeGraphRecordId,
    pub metadata: JsonMap<String, JsonValue>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApiKnowledgeGraph {
    pub nodes: Vec<ApiKnowledgeGraphNode>,
    pub edges: Vec<ApiKnowledgeGraphEdge>,
}

impl From<KnowledgeGraphNodeRow> for ApiKnowledgeGraphNode {
    fn from(value: KnowledgeGraphNodeRow) -> Self {
        Self {
            id: record_id_to_api(value.id),
            kind: value.kind,
            node_id: value.node_id,
            label: value.label,
            summary: value.summary,
            content: value.content,
            subtype: value.subtype,
            tags: value.tags,
            source: value.source,
            metadata: surreal_btreemap_to_json_map(value.metadata.unwrap_or_default()),
            updated_at: surreal_datetime_value_to_string(value.updated_at),
            read_by_search_context: {
                let s = surreal_datetime_value_to_string(value.read_by_search_context);
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            },
        }
    }
}

impl From<KnowledgeGraphEdgeRow> for ApiKnowledgeGraphEdge {
    fn from(value: KnowledgeGraphEdgeRow) -> Self {
        let in_record = value
            .in_record
            .unwrap_or_else(|| RecordId::new("kg_node", "missing"));
        let out = value
            .out
            .unwrap_or_else(|| RecordId::new("kg_node", "missing"));
        Self {
            id: record_id_to_api(value.id),
            relation: value.relation,
            in_record: record_id_to_api(in_record),
            out: record_id_to_api(out),
            metadata: surreal_btreemap_to_json_map(value.metadata.unwrap_or_default()),
            created_at: value
                .created_at
                .map(Some)
                .map(surreal_datetime_value_to_string)
                .unwrap_or_default(),
        }
    }
}

pub(crate) fn surreal_datetime_value_to_string(value: Option<surrealdb::types::Value>) -> String {
    match value {
        Some(surrealdb::types::Value::Datetime(value)) => value.to_string(),
        Some(surrealdb::types::Value::String(value)) => value.as_str().to_string(),
        Some(other) => {
            let json = other.into_json_value();
            if let Some(text) = json.as_str() {
                return text.to_string();
            }
            json.to_string()
        }
        None => String::new(),
    }
}

fn record_id_to_api(record_id: RecordId) -> ApiKnowledgeGraphRecordId {
    ApiKnowledgeGraphRecordId {
        tb: record_id.table.to_string(),
        id: record_id_key_to_json(record_id.key),
    }
}

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

fn surreal_object_to_json_map(value: Object) -> JsonMap<String, JsonValue> {
    value
        .into_iter()
        .map(|(key, value)| (key, surreal_value_to_json(value)))
        .collect()
}

fn surreal_btreemap_to_json_map(
    value: BTreeMap<String, SurrealDataValue>,
) -> JsonMap<String, JsonValue> {
    value
        .into_iter()
        .map(|(key, value)| (key, surreal_value_to_json(value)))
        .collect()
}

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
