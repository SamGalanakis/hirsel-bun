use lash::ToolResult;
use serde_json::{Map, Value};

pub(crate) fn edit_result_with(
    summary: impl Into<String>,
    mut fields: Map<String, Value>,
) -> ToolResult {
    fields.insert(
        "__type__".to_string(),
        Value::String("edit_result".to_string()),
    );
    fields.insert("summary".to_string(), Value::String(summary.into()));
    ToolResult::ok(Value::Object(fields))
}
