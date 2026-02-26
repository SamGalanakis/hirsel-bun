//! Shepherd chat session commands.
//!
//! Replaces legacy Shepherd session command surface with Shepherd-oriented
//! session lifecycle and history APIs.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use base64::Engine;
use lash_core::provider::Provider;
use lash_core::tools::hashline::{self, HashlineEdit};
use lash_core::{
    AgentCapabilities, AgentEvent, AgentStateEnvelope, EventSink, FsInstructionSource, HostProfile,
    InputItem, Message, MessageRole, OutputState, Part, PartKind, PromptOverrideMode,
    PromptSectionName, PromptSectionOverride, PruneState, RuntimeConfig, RuntimeEngine,
    ToolDefinition, ToolParam, ToolProvider, ToolResult, TurnInput, TurnStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::Emitter;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use super::delta;
use super::ResultExt;
use crate::core::board::mcp::BoardMcpServer;
use crate::core::delta::{DeltaState, UpdateBoardNodeRequest};
use crate::core::llm_provider;
use crate::core::mcp::McpToolServer;
use crate::core::orchestrator::create_orchestrator;
use crate::core::{
    Config, ProjectStore, RouteFiles, RouteStore, SQLiteState, ShepherdChatMessage,
    ShepherdChatStore,
};

struct ShepherdSession {
    scope: ShepherdScope,
    active_turn: Option<CancellationToken>,
    runtime: Option<RuntimeEngine>,
}

static ACTIVE_SESSIONS: OnceLock<StdMutex<HashMap<String, ShepherdSession>>> = OnceLock::new();

fn sessions() -> &'static StdMutex<HashMap<String, ShepherdSession>> {
    ACTIVE_SESSIONS.get_or_init(|| StdMutex::new(HashMap::new()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdTaskFocus {
    pub task_id: String,
    pub task_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ShepherdScope {
    #[serde(rename = "general")]
    General,
    #[serde(rename = "run")]
    Run {
        #[serde(rename = "runName")]
        run_name: String,
        #[serde(rename = "workspacePath", default)]
        workspace_path: String,
        #[serde(rename = "projectPath", default)]
        project_path: Option<String>,
    },
    #[serde(rename = "board")]
    Board {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "workspacePath", default)]
        workspace_path: Option<String>,
        #[serde(default)]
        focus: Option<ShepherdTaskFocus>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StartShepherdSessionRequest {
    #[serde(rename = "general")]
    General,
    #[serde(rename = "run")]
    Run {
        #[serde(rename = "runName")]
        run_name: String,
    },
    #[serde(rename = "board")]
    Board {
        #[serde(rename = "projectId")]
        project_id: i64,
    },
    #[serde(rename = "boardFocused")]
    BoardFocused {
        #[serde(rename = "projectId")]
        project_id: i64,
        #[serde(rename = "taskId")]
        task_id: String,
        #[serde(rename = "taskName")]
        task_name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartShepherdSessionResponse {
    pub session_id: String,
    pub scope: ShepherdScope,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ShepherdEvent {
    TextDelta {
        session_id: String,
        text: String,
    },
    ToolCallStart {
        session_id: String,
        tool_call_id: String,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        input: Option<String>,
    },
    ToolCallUpdate {
        session_id: String,
        tool_call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    Error {
        session_id: String,
        message: String,
    },
    MessageComplete {
        session_id: String,
    },
    SessionEnded {
        session_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ShepherdMessageChunk {
    Text {
        content: String,
    },
    Thinking {
        content: String,
    },
    Tool {
        id: String,
        title: String,
        #[serde(default)]
        kind: Option<String>,
        status: String,
        #[serde(default)]
        input: Option<String>,
        #[serde(default)]
        output: Option<String>,
    },
    Image {
        #[serde(rename = "mimeType")]
        mime_type: String,
        #[serde(rename = "dataBase64")]
        data_base64: String,
        #[serde(default)]
        name: Option<String>,
    },
}

const MAX_IMAGE_COUNT: usize = 8;
const MAX_IMAGE_BASE64_CHARS: usize = 12 * 1024 * 1024; // ~9MB raw bytes
const RUNTIME_HISTORY_LIMIT: usize = 48;
const RUNTIME_PREVIEW_MAX_CHARS: usize = 1200;
const NODE_READ_DEFAULT_LIMIT: usize = 2000;
const NODE_READ_MAX_LINE_LEN: usize = 2000;
const NODE_TRUNCATION_HINT: &str = "Pass `limit=null` for all lines.";

#[derive(Default)]
struct AssistantDraft {
    text: String,
    thinking: String,
    tools: Vec<ShepherdMessageChunk>,
    runtime_output: String,
    errored: bool,
}

struct ShepherdToolProvider {
    board: Option<StdMutex<BoardMcpServer>>,
    default_project_id: Option<i64>,
    active_route_id: StdMutex<Option<i64>>,
}

impl ShepherdToolProvider {
    fn new(default_project_id: Option<i64>, default_route_id: Option<i64>) -> Self {
        let board = default_project_id
            .map(BoardMcpServer::new)
            .map(StdMutex::new);
        Self {
            board,
            default_project_id,
            active_route_id: StdMutex::new(default_route_id),
        }
    }

    fn arg_i64(args: &Value, key: &str) -> Option<i64> {
        args.get(key).and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_u64().map(|n| n as i64))
                .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
        })
    }

    fn resolve_project_id(&self, args: &Value) -> Result<i64, String> {
        if let Some(project_id) = Self::arg_i64(args, "project_id") {
            return Ok(project_id);
        }
        self.default_project_id
            .ok_or_else(|| "project_id is required outside board/run scope".to_string())
    }

    fn resolve_route_id(&self, args: &Value) -> Result<i64, String> {
        if let Some(route_id) = Self::arg_i64(args, "route_id") {
            return Ok(route_id);
        }
        self.active_route_id
            .lock()
            .ok()
            .and_then(|g| *g)
            .ok_or_else(|| {
                "route_id is required (call board_routes first if you are unsure)".to_string()
            })
    }

    fn schema_type_to_param(ty: &str) -> &'static str {
        match ty {
            "integer" => "int",
            "number" => "float",
            "boolean" => "bool",
            "array" => "list",
            "object" => "dict",
            _ => "str",
        }
    }

    fn mcp_schema_params(schema: &Value) -> Vec<ToolParam> {
        let required: HashSet<String> = schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                    .collect()
            })
            .unwrap_or_default();

        schema
            .get("properties")
            .and_then(|v| v.as_object())
            .map(|props| {
                let mut entries = props
                    .iter()
                    .map(|(name, prop)| {
                        let param_type = prop
                            .get("type")
                            .and_then(|v| v.as_str())
                            .map(Self::schema_type_to_param)
                            .unwrap_or("str");
                        (name.clone(), required.contains(name), param_type)
                    })
                    .collect::<Vec<_>>();

                // Python wrappers require required args before optional defaults.
                entries.sort_by(|a, b| {
                    b.1.cmp(&a.1) // required=true first
                        .then_with(|| a.0.cmp(&b.0))
                });

                entries
                    .into_iter()
                    .map(|(name, is_required, param_type)| {
                        if is_required {
                            ToolParam::typed(&name, param_type)
                        } else {
                            ToolParam::optional(&name, param_type)
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    fn parse_offset(args: &Value) -> usize {
        args.get("offset")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(1)
            .max(1)
    }

    fn parse_limit(args: &Value) -> Result<Option<usize>, ToolResult> {
        match args.get("limit") {
            None => Ok(Some(NODE_READ_DEFAULT_LIMIT)),
            Some(v) if v.is_null() => Ok(None),
            Some(v) => {
                if let Some(s) = v.as_str() {
                    if s.eq_ignore_ascii_case("none") {
                        return Ok(None);
                    }
                    return Err(ToolResult::err_fmt(format_args!(
                        "Invalid limit: expected int, null, or \"none\""
                    )));
                }
                let n = match v.as_u64() {
                    Some(n) => n,
                    None => {
                        return Err(ToolResult::err_fmt(format_args!(
                            "Invalid limit: expected int, null, or \"none\""
                        )));
                    }
                };

                if n == 0 {
                    return Err(ToolResult::err_fmt(format_args!(
                        "Invalid limit: must be >= 1, or use null/\"none\" for no cap"
                    )));
                }
                Ok(Some(n as usize))
            }
        }
    }

    fn parse_hashline_edits(edits: &[Value]) -> Result<Vec<HashlineEdit>, String> {
        edits
            .iter()
            .enumerate()
            .map(|(idx, edit)| {
                Self::parse_single_hashline_edit(edit)
                    .map_err(|e| format!("Edit #{}: {}", idx + 1, e))
            })
            .collect()
    }

    fn parse_single_hashline_edit(edit: &Value) -> Result<HashlineEdit, String> {
        if let Some(obj) = edit.get("set_line") {
            let anchor = obj
                .get("anchor")
                .and_then(|v| v.as_str())
                .ok_or("set_line: missing 'anchor'")?
                .to_string();
            let new_text = obj
                .get("new_text")
                .and_then(|v| v.as_str())
                .ok_or("set_line: missing 'new_text'")?
                .to_string();
            return Ok(HashlineEdit::SetLine { anchor, new_text });
        }

        if let Some(obj) = edit.get("replace_lines") {
            let start_anchor = obj
                .get("start_anchor")
                .and_then(|v| v.as_str())
                .ok_or("replace_lines: missing 'start_anchor'")?
                .to_string();
            let end_anchor = obj
                .get("end_anchor")
                .and_then(|v| v.as_str())
                .ok_or("replace_lines: missing 'end_anchor'")?
                .to_string();
            let new_text = obj
                .get("new_text")
                .and_then(|v| v.as_str())
                .ok_or("replace_lines: missing 'new_text'")?
                .to_string();
            return Ok(HashlineEdit::ReplaceLines {
                start_anchor,
                end_anchor,
                new_text,
            });
        }

        if let Some(obj) = edit.get("insert_after") {
            let anchor = obj
                .get("anchor")
                .and_then(|v| v.as_str())
                .ok_or("insert_after: missing 'anchor'")?
                .to_string();
            let text = obj
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or("insert_after: missing 'text'")?
                .to_string();
            return Ok(HashlineEdit::InsertAfter { anchor, text });
        }

        if let Some(obj) = edit.get("replace") {
            let old_text = obj
                .get("old_text")
                .and_then(|v| v.as_str())
                .ok_or("replace: missing 'old_text'")?
                .to_string();
            let new_text = obj
                .get("new_text")
                .and_then(|v| v.as_str())
                .ok_or("replace: missing 'new_text'")?
                .to_string();
            let all = obj.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
            return Ok(HashlineEdit::Replace {
                old_text,
                new_text,
                all,
            });
        }

        Err(format!(
            "Unknown edit type. Expected one of: set_line, replace_lines, insert_after, replace. Got: {}",
            edit
        ))
    }

    fn append_truncation_notice(
        formatted: &mut String,
        start_idx: usize,
        end_idx: usize,
        total_lines: usize,
    ) {
        if end_idx >= total_lines {
            return;
        }
        let shown_start = start_idx + 1;
        let shown_end = end_idx;
        let omitted = total_lines - end_idx;
        let next_offset = end_idx + 1;
        formatted.push_str(&format!(
            "\n[results truncated: showing lines {}-{} of {} ({} more lines). Use offset={} to continue. {}]",
            shown_start, shown_end, total_lines, omitted, next_offset, NODE_TRUNCATION_HINT
        ));
    }

    fn compact_diff(old: &str, new: &str, target: &str, max_lines: usize) -> String {
        let diff = similar::TextDiff::from_lines(old, new);
        let unified = diff
            .unified_diff()
            .header(&format!("a/{target}"), &format!("b/{target}"))
            .to_string();
        if unified.is_empty() {
            return String::new();
        }
        let lines: Vec<&str> = unified.lines().collect();
        if lines.len() <= max_lines {
            unified
        } else {
            let mut truncated = lines[..max_lines].join("\n");
            truncated.push_str(&format!("\n... ({} more lines)", lines.len() - max_lines));
            truncated
        }
    }

    async fn read_node(&self, project_id: i64, route_id: i64, args: &Value) -> ToolResult {
        let node_id = match args.get("node_id").and_then(|v| v.as_str()).map(str::trim) {
            Some(id) if !id.is_empty() => id,
            _ => return ToolResult::err_fmt("Missing required parameter: node_id"),
        };

        let offset = Self::parse_offset(args);
        let limit = match Self::parse_limit(args) {
            Ok(v) => v,
            Err(error) => return error,
        };

        let state = DeltaState::with_route(project_id, route_id);
        let node = match state.get_node(node_id).await {
            Ok(node) => node,
            Err(_) => return ToolResult::err_fmt(format_args!("Node does not exist: {}", node_id)),
        };

        let lines: Vec<&str> = node.content.lines().collect();
        let total_lines = lines.len();
        let start_idx = (offset - 1).min(total_lines);
        let end_idx = match limit {
            Some(limit) => (start_idx + limit).min(total_lines),
            None => total_lines,
        };
        let selected: Vec<&str> = lines[start_idx..end_idx].to_vec();
        let truncated_content: String = selected
            .iter()
            .map(|line| {
                if line.len() > NODE_READ_MAX_LINE_LEN {
                    format!("{}...", &line[..NODE_READ_MAX_LINE_LEN])
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut formatted = hashline::format_hashlines(&truncated_content, offset);
        Self::append_truncation_notice(&mut formatted, start_idx, end_idx, total_lines);

        ToolResult::ok(json!(formatted))
    }

    async fn edit_node(&self, project_id: i64, route_id: i64, args: &Value) -> ToolResult {
        let node_id = match args.get("node_id").and_then(|v| v.as_str()).map(str::trim) {
            Some(id) if !id.is_empty() => id,
            _ => return ToolResult::err_fmt("Missing required parameter: node_id"),
        };
        let edits_json = match args.get("edits").and_then(|v| v.as_array()) {
            Some(edits) => edits,
            None => {
                return ToolResult::err(json!(
                    "Missing or invalid 'edits' parameter: expected a list"
                ));
            }
        };
        let edits = match Self::parse_hashline_edits(edits_json) {
            Ok(edits) => edits,
            Err(error) => return ToolResult::err(json!(error)),
        };

        let state = DeltaState::with_route(project_id, route_id);
        let node = match state.get_node(node_id).await {
            Ok(node) => node,
            Err(_) => return ToolResult::err_fmt(format_args!("Node does not exist: {}", node_id)),
        };

        let new_content = match hashline::apply_hashline_edits(&node.content, edits) {
            Ok(content) => content,
            Err(error) => return ToolResult::err(json!(error)),
        };

        match state
            .update_node(
                node_id,
                &UpdateBoardNodeRequest {
                    content: Some(new_content.clone()),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(updated) => ToolResult::ok(json!({
                "__type__": "edit_result",
                "summary": format!(
                    "Applied {} edit(s) to {} ({} lines)",
                    edits_json.len(),
                    updated.id,
                    updated.content.lines().count()
                ),
                "diff": Self::compact_diff(&node.content, &new_content, &updated.id, 50)
            })),
            Err(error) => ToolResult::err_fmt(format_args!("Failed to write node: {}", error)),
        }
    }

    async fn write_node(&self, project_id: i64, route_id: i64, args: &Value) -> ToolResult {
        let node_id = match args.get("node_id").and_then(|v| v.as_str()).map(str::trim) {
            Some(id) if !id.is_empty() => id,
            _ => return ToolResult::err_fmt("Missing required parameter: node_id"),
        };
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let state = DeltaState::with_route(project_id, route_id);
        match state
            .update_node(
                node_id,
                &UpdateBoardNodeRequest {
                    content: Some(content.clone()),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_updated) => ToolResult::ok(json!(format!(
                "Wrote {} bytes to {}",
                content.len(),
                node_id
            ))),
            Err(error) => ToolResult::err_fmt(format_args!("Failed to write node: {}", error)),
        }
    }

    async fn find_replace_node(&self, project_id: i64, route_id: i64, args: &Value) -> ToolResult {
        let node_id = match args.get("node_id").and_then(|v| v.as_str()).map(str::trim) {
            Some(id) if !id.is_empty() => id,
            _ => return ToolResult::err_fmt("Missing required parameter: node_id"),
        };
        let old_text = match args.get("old_text").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::err_fmt("Missing required parameter: old_text"),
        };
        let new_text = match args.get("new_text").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::err_fmt("Missing required parameter: new_text"),
        };
        let replace_all = args.get("all").and_then(|v| v.as_bool()).unwrap_or(false);

        let state = DeltaState::with_route(project_id, route_id);
        let node = match state.get_node(node_id).await {
            Ok(node) => node,
            Err(_) => return ToolResult::err_fmt(format_args!("Node does not exist: {}", node_id)),
        };

        if !node.content.contains(old_text) {
            return ToolResult::err_fmt("old_text not found in node");
        }

        let (new_content, count) = if replace_all {
            let count = node.content.matches(old_text).count();
            (node.content.replace(old_text, new_text), count)
        } else {
            (node.content.replacen(old_text, new_text, 1), 1)
        };

        match state
            .update_node(
                node_id,
                &UpdateBoardNodeRequest {
                    content: Some(new_content),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(updated) => {
                let label = if count == 1 {
                    "1 replacement".to_string()
                } else {
                    format!("{} replacements", count)
                };
                ToolResult::ok(json!({
                    "__type__": "edit_result",
                    "summary": format!("{} made in {}", label, updated.id),
                    "diff": Self::compact_diff(&node.content, &updated.content, &updated.id, 50),
                }))
            }
            Err(error) => ToolResult::err_fmt(format_args!("Failed to write node: {}", error)),
        }
    }

    async fn resolve_run_name(&self, project_id: i64, route_id: i64) -> Result<String, ToolResult> {
        match delta::get_project_run(project_id, route_id).await {
            Ok(Some(run)) => Ok(run.run_name),
            Ok(None) => Err(ToolResult::err(json!({
                "error": "No run exists for this route. Call shepherd_start_run first."
            }))),
            Err(error) => Err(ToolResult::err(json!({ "error": error }))),
        }
    }

    async fn shepherd_get_workers(&self, project_id: i64, route_id: i64) -> ToolResult {
        let run_name = match self.resolve_run_name(project_id, route_id).await {
            Ok(name) => name,
            Err(error) => return error,
        };

        let orch = match create_orchestrator(None) {
            Ok(orch) => orch,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        match orch.list_workers(&run_name).await {
            Ok(workers) => ToolResult::ok(json!({
                "run_name": run_name,
                "workers": workers
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn shepherd_get_worker_events(
        &self,
        project_id: i64,
        route_id: i64,
        args: &Value,
    ) -> ToolResult {
        let worker_name = match args
            .get("worker_name")
            .and_then(|v| v.as_str())
            .map(str::trim)
        {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => return ToolResult::err_fmt("Missing required parameter: worker_name"),
        };
        let after_id = Self::arg_i64(args, "after_id");
        let limit = Self::arg_i64(args, "limit");

        let run_name = match self.resolve_run_name(project_id, route_id).await {
            Ok(name) => name,
            Err(error) => return error,
        };

        let orch = match create_orchestrator(None) {
            Ok(orch) => orch,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        match orch
            .get_worker_events(&run_name, &worker_name, after_id, limit)
            .await
        {
            Ok(resp) => ToolResult::ok(json!({
                "run_name": run_name,
                "worker_name": worker_name,
                "events": resp.events,
                "last_id": resp.last_id,
                "worker_status": resp.worker_status
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    fn board_definitions(&self) -> Vec<ToolDefinition> {
        let Some(board) = &self.board else {
            return Vec::new();
        };
        let Ok(board) = board.lock() else {
            return Vec::new();
        };
        board
            .tools()
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.name.to_string(),
                description: tool.description.to_string(),
                params: Self::mcp_schema_params(&tool.input_schema),
                returns: "dict".to_string(),
                examples: vec![],
                hidden: false,
                inject_into_prompt: true,
            })
            .collect()
    }

    fn execute_board_tool(&self, name: &str, args: &Value) -> ToolResult {
        let Some(board) = &self.board else {
            return ToolResult::err(json!({
                "error": "Board tools require a run or board scoped Shepherd session"
            }));
        };
        let Ok(mut board) = board.lock() else {
            return ToolResult::err(json!({"error": "Failed to lock board tool server"}));
        };

        match board.execute(name, args.clone()) {
            Ok((output, _)) => {
                let parsed_output =
                    serde_json::from_str::<Value>(&output).unwrap_or_else(|_| json!(output));

                if name == "board_switch_route" || name == "board_set_active_route" {
                    let route_id = if name == "board_switch_route" {
                        Self::arg_i64(args, "route_id")
                    } else {
                        parsed_output
                            .get("route")
                            .and_then(|r| r.get("id"))
                            .and_then(|id| {
                                id.as_i64()
                                    .or_else(|| id.as_u64().map(|n| n as i64))
                                    .or_else(|| id.as_str().and_then(|s| s.parse::<i64>().ok()))
                            })
                    };
                    if let Some(route_id) = route_id {
                        if let Ok(mut active) = self.active_route_id.lock() {
                            *active = Some(route_id);
                        }
                    }
                }

                ToolResult::ok(parsed_output)
            }
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }
}

#[async_trait::async_trait]
impl ToolProvider for ShepherdToolProvider {
    fn definitions(&self) -> Vec<ToolDefinition> {
        let mut defs = self.board_definitions();
        defs.extend([
            ToolDefinition {
                name: "shepherd_start_run".to_string(),
                description: "Dispatch board nodes and start/continue execution for this route."
                    .to_string(),
                params: vec![
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                hidden: false,
                inject_into_prompt: true,
            },
            ToolDefinition {
                name: "shepherd_get_project_run".to_string(),
                description: "Get current run metadata for a project route.".to_string(),
                params: vec![
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                hidden: false,
                inject_into_prompt: true,
            },
            ToolDefinition {
                name: "shepherd_get_workers".to_string(),
                description: "List workers for the current run on this route.".to_string(),
                params: vec![
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                hidden: false,
                inject_into_prompt: true,
            },
            ToolDefinition {
                name: "shepherd_get_worker_events".to_string(),
                description: "Get worker output/tool events for a worker in the current run."
                    .to_string(),
                params: vec![
                    ToolParam::typed("worker_name", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_id", "int"),
                    ToolParam::optional("after_id", "int"),
                    ToolParam::optional("limit", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                hidden: false,
                inject_into_prompt: true,
            },
            ToolDefinition {
                name: "read_node".to_string(),
                description:
                    "Read node content (hashline format) using node_id instead of file path."
                        .to_string(),
                params: vec![
                    ToolParam::typed("node_id", "str"),
                    ToolParam::optional("offset", "int"),
                    ToolParam::optional("limit", "int"),
                ],
                returns: "str".to_string(),
                examples: vec![],
                hidden: false,
                inject_into_prompt: true,
            },
            ToolDefinition {
                name: "edit_node".to_string(),
                description: "Apply hashline edits to node content using node_id.".to_string(),
                params: vec![
                    ToolParam::typed("node_id", "str"),
                    ToolParam::typed("edits", "list"),
                ],
                returns: "EditResult".to_string(),
                examples: vec![],
                hidden: false,
                inject_into_prompt: true,
            },
            ToolDefinition {
                name: "write_node".to_string(),
                description: "Write full node content using node_id.".to_string(),
                params: vec![
                    ToolParam::typed("node_id", "str"),
                    ToolParam::typed("content", "str"),
                ],
                returns: "str".to_string(),
                examples: vec![],
                hidden: false,
                inject_into_prompt: true,
            },
            ToolDefinition {
                name: "find_replace_node".to_string(),
                description: "Exact text replacement in node content.".to_string(),
                params: vec![
                    ToolParam::typed("node_id", "str"),
                    ToolParam::typed("old_text", "str"),
                    ToolParam::typed("new_text", "str"),
                    ToolParam::optional("all", "bool"),
                ],
                returns: "EditResult".to_string(),
                examples: vec![],
                hidden: false,
                inject_into_prompt: true,
            },
            ToolDefinition {
                name: "shepherd_get_board_tree".to_string(),
                description: "Return the full board tree for a project route.".to_string(),
                params: vec![
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                hidden: false,
                inject_into_prompt: true,
            },
        ]);
        defs
    }

    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        if name.starts_with("board_") {
            return self.execute_board_tool(name, args);
        }

        let project_id = match self.resolve_project_id(args) {
            Ok(v) => v,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };
        let route_id = match self.resolve_route_id(args) {
            Ok(v) => v,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };

        match name {
            "shepherd_start_run" => match delta::start_shepherd_run(project_id, route_id).await {
                Ok(resp) => ToolResult::ok(json!(resp)),
                Err(error) => ToolResult::err(json!({ "error": error })),
            },
            "shepherd_get_project_run" => {
                match delta::get_project_run(project_id, route_id).await {
                    Ok(resp) => ToolResult::ok(json!(resp)),
                    Err(error) => ToolResult::err(json!({ "error": error })),
                }
            }
            "shepherd_get_workers" => self.shepherd_get_workers(project_id, route_id).await,
            "shepherd_get_worker_events" => {
                self.shepherd_get_worker_events(project_id, route_id, args)
                    .await
            }
            "read_node" => self.read_node(project_id, route_id, args).await,
            "edit_node" => self.edit_node(project_id, route_id, args).await,
            "write_node" => self.write_node(project_id, route_id, args).await,
            "find_replace_node" => self.find_replace_node(project_id, route_id, args).await,
            "shepherd_get_board_tree" => match delta::get_board_tree(project_id, route_id).await {
                Ok(resp) => ToolResult::ok(json!(resp)),
                Err(error) => ToolResult::err(json!({ "error": error })),
            },
            _ => ToolResult::err(json!({ "error": format!("Unknown tool: {}", name) })),
        }
    }
}

struct ShepherdLashSink {
    app: tauri::AppHandle,
    session_id: String,
    tool_seq: AtomicU64,
    draft: Arc<Mutex<AssistantDraft>>,
}

impl ShepherdLashSink {
    fn new(app: tauri::AppHandle, session_id: String, draft: Arc<Mutex<AssistantDraft>>) -> Self {
        Self {
            app,
            session_id,
            draft,
            tool_seq: AtomicU64::new(1),
        }
    }

    fn emit(&self, event: &ShepherdEvent) {
        if let Err(e) = self.app.emit("shepherd-event", (&self.session_id, event)) {
            warn!("failed to emit shepherd-event: {}", e);
        }
    }

    fn tool_title_kind(name: &str) -> (String, Option<String>) {
        let mapped = match name {
            "board_routes" => ("Board Routes".to_string(), Some("search".to_string())),
            "board_switch_route" => ("Switch Route".to_string(), Some("execute".to_string())),
            "board_create_route" => ("Create Route".to_string(), Some("execute".to_string())),
            "board_set_active_route" => {
                ("Set Active Route".to_string(), Some("execute".to_string()))
            }
            "board_view" | "shepherd_get_board_tree" => {
                ("Board View".to_string(), Some("search".to_string()))
            }
            "board_feature" | "board_task" | "board_check" | "board_delete"
            | "board_requeue_node" => ("Board Edit".to_string(), Some("edit".to_string())),
            "read_node" => ("Node Read".to_string(), Some("read".to_string())),
            "edit_node" | "find_replace_node" => {
                ("Node Edit".to_string(), Some("edit".to_string()))
            }
            "write_node" => ("Node Write".to_string(), Some("write".to_string())),
            "shepherd_start_run" => ("Start Run".to_string(), Some("execute".to_string())),
            "shepherd_get_project_run" => ("Run Status".to_string(), Some("search".to_string())),
            "shepherd_get_workers" => ("Workers".to_string(), Some("search".to_string())),
            "shepherd_get_worker_events" => {
                ("Worker Events".to_string(), Some("search".to_string()))
            }
            _ => (name.to_string(), None),
        };
        mapped
    }

    fn is_repl_fragment_only(text: &str) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() || !trimmed.contains('<') {
            return false;
        }

        trimmed.chars().all(|c| {
            matches!(
                c.to_ascii_lowercase(),
                '<' | '>' | '/' | 'r' | 'e' | 'p' | 'l' | ' '
            )
        })
    }

    fn sanitize_assistant_text(text: &str) -> String {
        let out = text.replace("</repl>", "").replace("<repl>", "");
        if Self::is_repl_fragment_only(&out) {
            String::new()
        } else {
            out
        }
    }
}

#[async_trait::async_trait]
impl EventSink for ShepherdLashSink {
    async fn emit(&self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta { content } => {
                let sanitized = Self::sanitize_assistant_text(&content);
                if sanitized.is_empty() {
                    return;
                }
                let mut draft = self.draft.lock().await;
                draft.text.push_str(&sanitized);
            }
            AgentEvent::CodeBlock { code } => {
                let _ = code;
            }
            AgentEvent::CodeOutput { output, error } => {
                let mut draft = self.draft.lock().await;
                if !output.trim().is_empty() {
                    if !draft.runtime_output.is_empty() {
                        draft.runtime_output.push('\n');
                    }
                    draft.runtime_output.push_str(&output);
                }
                if let Some(err) = error {
                    if !err.trim().is_empty() {
                        if !draft.runtime_output.is_empty() {
                            draft.runtime_output.push('\n');
                        }
                        draft
                            .runtime_output
                            .push_str(&format!("Runtime error: {}", err));
                    }
                }
            }
            AgentEvent::ToolCall {
                name,
                args,
                result,
                success,
                ..
            } => {
                let tool_call_id = format!(
                    "shepherd-tool-{}",
                    self.tool_seq.fetch_add(1, Ordering::Relaxed)
                );
                let (title, kind) = Self::tool_title_kind(&name);
                let input = serde_json::to_string(&args).ok();
                let output = serde_json::to_string(&result).ok();

                self.emit(&ShepherdEvent::ToolCallStart {
                    session_id: self.session_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    title: title.clone(),
                    kind: kind.clone(),
                    input: input.clone(),
                });

                let status = if success { "completed" } else { "failed" }.to_string();
                self.emit(&ShepherdEvent::ToolCallUpdate {
                    session_id: self.session_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    title: Some(title.clone()),
                    status: status.clone(),
                    output: output.clone(),
                });

                let mut draft = self.draft.lock().await;
                draft.tools.push(ShepherdMessageChunk::Tool {
                    id: tool_call_id,
                    title,
                    kind,
                    status,
                    input,
                    output,
                });
            }
            AgentEvent::Message { text, kind } => {
                if kind == "final" {
                    let sanitized_final = Self::sanitize_assistant_text(&text);
                    let mut draft = self.draft.lock().await;
                    if draft.text.trim().is_empty() && !sanitized_final.trim().is_empty() {
                        draft.text.push_str(sanitized_final.trim());
                    }
                }
            }
            AgentEvent::Error { message, .. } => {
                self.emit(&ShepherdEvent::Error {
                    session_id: self.session_id.clone(),
                    message: message.clone(),
                });
                let mut draft = self.draft.lock().await;
                draft.errored = true;
            }
            AgentEvent::Prompt { .. }
            | AgentEvent::LlmRequest { .. }
            | AgentEvent::LlmResponse { .. }
            | AgentEvent::TokenUsage { .. }
            | AgentEvent::RetryStatus { .. }
            | AgentEvent::SubAgentDone { .. }
            | AgentEvent::Done => {}
        }
    }
}

fn validate_chunks(chunks: &[ShepherdMessageChunk]) -> Result<(), String> {
    if chunks.is_empty() {
        return Err("message must contain at least one chunk".to_string());
    }

    let mut image_count = 0usize;
    for chunk in chunks {
        match chunk {
            ShepherdMessageChunk::Text { content } | ShepherdMessageChunk::Thinking { content } => {
                if content.trim().is_empty() {
                    return Err("text/thinking chunk content cannot be empty".to_string());
                }
            }
            ShepherdMessageChunk::Tool {
                id, title, status, ..
            } => {
                if id.trim().is_empty() || title.trim().is_empty() || status.trim().is_empty() {
                    return Err("tool chunk requires non-empty id/title/status".to_string());
                }
            }
            ShepherdMessageChunk::Image {
                mime_type,
                data_base64,
                ..
            } => {
                image_count += 1;
                if !mime_type.starts_with("image/") {
                    return Err(format!("invalid image mime type: {}", mime_type));
                }
                if data_base64.is_empty() {
                    return Err("image chunk dataBase64 cannot be empty".to_string());
                }
                if data_base64.len() > MAX_IMAGE_BASE64_CHARS {
                    return Err("image too large for Shepherd message".to_string());
                }
            }
        }
    }

    if image_count > MAX_IMAGE_COUNT {
        return Err(format!(
            "too many images in one message (max {})",
            MAX_IMAGE_COUNT
        ));
    }

    Ok(())
}

fn chunks_to_json(chunks: &[ShepherdMessageChunk]) -> Result<String, String> {
    validate_chunks(chunks)?;
    serde_json::to_string(chunks).map_err(|e| format!("failed to serialize chunks: {}", e))
}

fn build_user_chunks(
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
) -> Result<Vec<ShepherdMessageChunk>, String> {
    if let Some(chunks) = chunks {
        validate_chunks(&chunks)?;
        return Ok(chunks);
    }

    let text = content.unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return Err("message content is empty".to_string());
    }

    Ok(vec![ShepherdMessageChunk::Text { content: text }])
}

fn chunk_text(chunks: &[ShepherdMessageChunk]) -> String {
    chunks
        .iter()
        .filter_map(|chunk| match chunk {
            ShepherdMessageChunk::Text { content } => Some(content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn chunk_image_count(chunks: &[ShepherdMessageChunk]) -> usize {
    chunks
        .iter()
        .filter(|c| matches!(c, ShepherdMessageChunk::Image { .. }))
        .count()
}

fn decode_png_images(chunks: &[ShepherdMessageChunk]) -> Result<Vec<Vec<u8>>, String> {
    let mut images = Vec::new();

    for chunk in chunks {
        let ShepherdMessageChunk::Image {
            mime_type,
            data_base64,
            ..
        } = chunk
        else {
            continue;
        };

        if mime_type != "image/png" {
            return Err(format!(
                "Shepherd currently supports pasted PNG images only (got {})",
                mime_type
            ));
        }

        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data_base64)
            .map_err(|e| format!("invalid image dataBase64: {}", e))?;
        images.push(decoded);
    }

    Ok(images)
}

fn parse_chunks_from_json(chunks_json: &str) -> Vec<ShepherdMessageChunk> {
    serde_json::from_str(chunks_json).unwrap_or_default()
}

fn truncate_for_runtime(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let count = trimmed.chars().count();
    if count <= max_chars {
        return trimmed.to_string();
    }

    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

fn summarize_chunks_for_runtime(chunks: &[ShepherdMessageChunk]) -> String {
    let mut text_parts: Vec<String> = Vec::new();
    let mut image_count = 0usize;
    let mut tool_notes: Vec<String> = Vec::new();

    for chunk in chunks {
        match chunk {
            ShepherdMessageChunk::Text { content } => {
                let t = content.trim();
                if !t.is_empty() {
                    text_parts.push(t.to_string());
                }
            }
            ShepherdMessageChunk::Tool { title, status, .. } => {
                tool_notes.push(format!("{}({})", title, status));
            }
            ShepherdMessageChunk::Image { .. } => {
                image_count += 1;
            }
            ShepherdMessageChunk::Thinking { .. } => {}
        }
    }

    let mut out = text_parts.join("\n\n");

    if image_count > 0 {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!(
            "[{} image attachment{}]",
            image_count,
            if image_count == 1 { "" } else { "s" }
        ));
    }

    if !tool_notes.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("[tool activity: ");
        out.push_str(&tool_notes.join(", "));
        out.push(']');
    }

    truncate_for_runtime(&out, RUNTIME_PREVIEW_MAX_CHARS)
}

fn history_role_to_message_role(role: &str) -> Option<MessageRole> {
    match role {
        "user" => Some(MessageRole::User),
        "assistant" => Some(MessageRole::Assistant),
        "system" => Some(MessageRole::System),
        _ => None,
    }
}

fn build_runtime_messages(history: &[ShepherdChatMessage]) -> Vec<Message> {
    let mut messages = Vec::with_capacity(history.len());

    for item in history {
        let Some(role) = history_role_to_message_role(item.role.as_str()) else {
            continue;
        };

        let summary = summarize_chunks_for_runtime(&parse_chunks_from_json(&item.chunks_json));
        if summary.is_empty() {
            continue;
        }

        let message_id = format!("m{}", messages.len());
        messages.push(Message {
            id: message_id.clone(),
            role,
            parts: vec![Part {
                id: format!("{}.p0", message_id),
                kind: PartKind::Text,
                content: summary,
                prune_state: PruneState::Intact,
            }],
        });
    }

    messages
}

fn build_scope(request: StartShepherdSessionRequest) -> ShepherdScope {
    match request {
        StartShepherdSessionRequest::General => ShepherdScope::General,
        StartShepherdSessionRequest::Run { run_name } => ShepherdScope::Run {
            run_name,
            workspace_path: String::new(),
            project_path: None,
        },
        StartShepherdSessionRequest::Board { project_id } => ShepherdScope::Board {
            project_id,
            workspace_path: None,
            focus: None,
        },
        StartShepherdSessionRequest::BoardFocused {
            project_id,
            task_id,
            task_name,
        } => ShepherdScope::Board {
            project_id,
            workspace_path: None,
            focus: Some(ShepherdTaskFocus { task_id, task_name }),
        },
    }
}

fn scope_label(scope: &ShepherdScope) -> String {
    match scope {
        ShepherdScope::General => "general".to_string(),
        ShepherdScope::Run { run_name, .. } => format!("run:{}", run_name),
        ShepherdScope::Board { project_id, .. } => format!("board:{}", project_id),
    }
}

fn build_scope_guidance(
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    cwd: &Path,
) -> String {
    let focus_line = match focus {
        Some(f) => format!("Focus node: {} ({})", f.task_name, f.task_id),
        None => "Focus node: none".to_string(),
    };

    format!(
        "## Hirsel Shepherd Scope\n\n\
        Scope: {}\n\
        {}\n\
        Workspace root: {}\n\n\
        ## Hirsel Constraints\n\n\
        - For board edits/execution work, use tools; do not edit board files directly.\n\
        - For simple conversational questions, answer directly in plain language without REPL code.\n\
        - For board node content edits, use only `read_node`, `edit_node`, `write_node`, and `find_replace_node`.\n\
        - Never claim work happened unless you actually executed tools.\n\
        - Never return raw tool payloads (JSON/Python dict/list) as final user-facing output.\n\
        - Summarize tool outcomes in plain language.\n\
        - For create/setup/scaffold/build/implement requests, perform at least one mutating board operation before finishing.",
        scope_label(scope),
        focus_line,
        cwd.display()
    )
}

fn shepherd_prompt_overrides(
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    cwd: &Path,
) -> Vec<PromptSectionOverride> {
    vec![PromptSectionOverride {
        section: PromptSectionName::ProjectInstructions,
        mode: PromptOverrideMode::Append,
        content: build_scope_guidance(scope, focus, cwd),
    }]
}

fn build_user_turn_text(chunks: &[ShepherdMessageChunk]) -> String {
    let text = chunk_text(chunks).trim().to_string();
    if !text.is_empty() {
        return text;
    }

    let image_count = chunk_image_count(chunks);
    if image_count > 0 {
        return format!(
            "Please inspect the {} attached image{} and help based on what you observe.",
            image_count,
            if image_count == 1 { "" } else { "s" }
        );
    }

    "Continue.".to_string()
}

fn looks_like_runtime_traceback(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    t.contains("Traceback (most recent call last):")
        || t.contains("Runtime error:")
        || t.contains("NameError:")
        || t.contains("File \"repl_")
}

fn build_assistant_chunks(draft: &AssistantDraft, final_text: &str) -> Vec<ShepherdMessageChunk> {
    let mut chunks = Vec::new();

    if !draft.thinking.trim().is_empty() {
        chunks.push(ShepherdMessageChunk::Thinking {
            content: draft.thinking.clone(),
        });
    }

    let sanitized_text = ShepherdLashSink::sanitize_assistant_text(final_text);
    if !sanitized_text.trim().is_empty() {
        chunks.push(ShepherdMessageChunk::Text {
            content: sanitized_text,
        });
    }

    chunks.extend(draft.tools.clone());
    chunks
}

async fn load_scope_messages(
    scope: &ShepherdScope,
    limit: usize,
) -> Result<Vec<ShepherdChatMessage>, String> {
    let store = ShepherdChatStore::open().await.str_err()?;

    let mut messages = match scope {
        ShepherdScope::General => store.get_messages(None).await.str_err()?,
        ShepherdScope::Run { run_name, .. } => {
            store.get_messages(Some(run_name)).await.str_err()?
        }
        ShepherdScope::Board { project_id, .. } => store
            .get_board_messages(*project_id, limit)
            .await
            .str_err()?,
    };

    if !matches!(scope, ShepherdScope::Board { .. }) && messages.len() > limit {
        let start = messages.len().saturating_sub(limit);
        messages = messages.split_off(start);
    }

    Ok(messages)
}

async fn save_message(scope: &ShepherdScope, role: &str, chunks_json: &str) -> Result<i64, String> {
    let store = ShepherdChatStore::open().await.str_err()?;
    match scope {
        ShepherdScope::General => store.save_message(None, role, chunks_json).await.str_err(),
        ShepherdScope::Run { run_name, .. } => store
            .save_message(Some(run_name), role, chunks_json)
            .await
            .str_err(),
        ShepherdScope::Board { project_id, .. } => store
            .save_board_message(*project_id, role, chunks_json)
            .await
            .str_err(),
    }
}

async fn resolve_scope_project_id(scope: &ShepherdScope) -> Option<i64> {
    match scope {
        ShepherdScope::General => None,
        ShepherdScope::Board { project_id, .. } => Some(*project_id),
        ShepherdScope::Run { run_name, .. } => {
            let state = SQLiteState::new(run_name).await.ok()?;
            state.get_project_id().await.ok().flatten()
        }
    }
}

async fn resolve_scope_route_id(scope: &ShepherdScope) -> Option<i64> {
    match scope {
        ShepherdScope::General => None,
        ShepherdScope::Run { run_name, .. } => {
            let state = SQLiteState::new(run_name).await.ok()?;
            state.get_route_id().await.ok()
        }
        ShepherdScope::Board { project_id, .. } => {
            let project_store = ProjectStore::open().await.ok()?;
            let project = project_store.get_project(*project_id).await.ok()?;
            project.active_route_id
        }
    }
}

async fn resolve_scope_workspace(scope: &ShepherdScope) -> Option<PathBuf> {
    match scope {
        ShepherdScope::General => None,
        ShepherdScope::Run {
            run_name,
            workspace_path,
            project_path,
        } => {
            if !workspace_path.trim().is_empty() {
                return Some(PathBuf::from(workspace_path));
            }
            if let Some(path) = project_path.as_ref().filter(|p| !p.trim().is_empty()) {
                return Some(PathBuf::from(path));
            }
            if let Ok(state) = SQLiteState::new(run_name).await {
                if let Ok(Some(path)) = state.get_project_path().await {
                    return Some(PathBuf::from(path));
                }
            }
            None
        }
        ShepherdScope::Board {
            project_id,
            workspace_path,
            ..
        } => {
            if let Some(path) = workspace_path.as_ref().filter(|p| !p.trim().is_empty()) {
                return Some(PathBuf::from(path));
            }

            let project_store = ProjectStore::open().await.ok()?;
            let project = project_store.get_project(*project_id).await.ok()?;
            let route_store = RouteStore::new(*project_id).await.ok()?;

            let route = if let Some(route_id) = project.active_route_id {
                match route_store.get_route(route_id).await {
                    Ok(route) => route,
                    Err(_) => route_store.create_main_route().await.ok()?,
                }
            } else {
                route_store.create_main_route().await.ok()?
            };

            Some(RouteFiles::new(*project_id, &route.name).route_dir())
        }
    }
}

fn resolve_runtime_cwd(path: Option<PathBuf>) -> PathBuf {
    let from_scope = path.filter(|p| p.exists() && p.is_dir());
    if let Some(path) = from_scope {
        return path;
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn shepherd_board_tool_names() -> &'static [&'static str] {
    &[
        "board_routes",
        "board_switch_route",
        "board_create_route",
        "board_set_active_route",
        "board_view",
        "board_feature",
        "board_task",
        "board_check",
        "board_delete",
        "board_requeue_node",
        "read_node",
        "edit_node",
        "write_node",
        "find_replace_node",
        "shepherd_start_run",
        "shepherd_get_project_run",
        "shepherd_get_workers",
        "shepherd_get_worker_events",
        "shepherd_get_board_tree",
    ]
}

fn board_only_capabilities() -> AgentCapabilities {
    let enabled_tools = shepherd_board_tool_names()
        .iter()
        .map(|name| (*name).to_string())
        .collect::<BTreeSet<_>>();
    AgentCapabilities {
        enabled_capabilities: BTreeSet::new(),
        enabled_tools,
    }
}

async fn load_shepherd_provider() -> Result<Provider, String> {
    let (config, _) = Config::load().map_err(|e| format!("failed to load config: {}", e))?;
    llm_provider::resolve_provider(&config).await
}

/// Start a Shepherd session.
#[tauri::command]
pub async fn start_shepherd_session(
    request: StartShepherdSessionRequest,
) -> Result<StartShepherdSessionResponse, String> {
    let scope = build_scope(request);
    let session_id = uuid::Uuid::new_v4().to_string();

    sessions()
        .lock()
        .map_err(|_| "failed to lock Shepherd session map".to_string())?
        .insert(
            session_id.clone(),
            ShepherdSession {
                scope: scope.clone(),
                active_turn: None,
                runtime: None,
            },
        );

    Ok(StartShepherdSessionResponse { session_id, scope })
}

/// Send a message to Shepherd and stream a lash-core response.
#[tauri::command]
pub async fn send_shepherd_message(
    app: tauri::AppHandle,
    session_id: String,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
    focus: Option<ShepherdTaskFocus>,
) -> Result<(), String> {
    let (scope, cancel, mut session_runtime) = {
        let mut guard = sessions()
            .lock()
            .map_err(|_| "failed to lock Shepherd session map".to_string())?;
        let session = guard
            .get_mut(&session_id)
            .ok_or_else(|| format!("Unknown Shepherd session: {}", session_id))?;

        if session.active_turn.is_some() {
            return Err("Shepherd session already has an active turn".to_string());
        }

        let cancel = CancellationToken::new();
        session.active_turn = Some(cancel.clone());
        (session.scope.clone(), cancel, session.runtime.take())
    };

    let result = async {
        let focus = focus.or_else(|| match &scope {
            ShepherdScope::Board { focus, .. } => focus.clone(),
            _ => None,
        });

        let user_chunks = build_user_chunks(content, chunks)?;
        let user_chunks_json = chunks_to_json(&user_chunks)?;
        let user_images_png = decode_png_images(&user_chunks)?;
        let user_turn_text = build_user_turn_text(&user_chunks);

        let history = if session_runtime.is_none() {
            load_scope_messages(&scope, RUNTIME_HISTORY_LIMIT).await?
        } else {
            Vec::new()
        };
        let cwd = resolve_runtime_cwd(resolve_scope_workspace(&scope).await);
        let scope_project_id = resolve_scope_project_id(&scope).await;
        let scope_route_id = resolve_scope_route_id(&scope).await;
        let prompt_overrides = shepherd_prompt_overrides(&scope, focus.as_ref(), &cwd);
        if session_runtime.is_none() {
            let tools: Arc<dyn ToolProvider> =
                Arc::new(ShepherdToolProvider::new(scope_project_id, scope_route_id));
            let provider = load_shepherd_provider().await?;

            let (model, reasoning_effort) = provider
                .default_agent_model("high")
                .map(|(m, effort)| (m.to_string(), effort.map(str::to_string)))
                .unwrap_or_else(|| {
                    let model = provider.default_model().to_string();
                    let effort = provider.reasoning_effort_for_model(&model).map(str::to_string);
                    (model, effort)
                });

            let runtime_config = RuntimeConfig {
                capabilities: board_only_capabilities(),
                model,
                provider,
                session_id: Some(session_id.clone()),
                max_context_tokens: None,
                include_soul: false,
                llm_log_path: None,
                headless: false,
                host_profile: HostProfile::Embedded,
                prompt_overrides,
                base_dir: Some(cwd.clone()),
                path_resolver: None,
                sanitizer: Default::default(),
                termination: Default::default(),
                instruction_source: Arc::new(FsInstructionSource::new()),
            };
            let mut state = AgentStateEnvelope::default();
            state.agent_id = format!("shepherd-{}", session_id);
            state.messages = build_runtime_messages(&history);

            let mut runtime = RuntimeEngine::from_state(runtime_config, tools, state)
                .await
                .map_err(|e| format!("failed to create shepherd lash runtime: {}", e))?;
            runtime.set_reasoning_effort(reasoning_effort);
            session_runtime = Some(runtime);
        }

        let runtime = session_runtime
            .as_mut()
            .ok_or_else(|| "failed to initialize shepherd runtime".to_string())?;

        save_message(&scope, "user", &user_chunks_json).await?;

        let draft = Arc::new(Mutex::new(AssistantDraft::default()));
        let sink = ShepherdLashSink::new(app.clone(), session_id.clone(), draft.clone());

        let mut turn_items = vec![InputItem::Text {
            text: user_turn_text.clone(),
        }];
        let mut image_blobs: HashMap<String, Vec<u8>> = HashMap::new();
        for (idx, bytes) in user_images_png.into_iter().enumerate() {
            let id = format!("image-{}", idx + 1);
            turn_items.push(InputItem::ImageRef { id: id.clone() });
            image_blobs.insert(id, bytes);
        }

        let turn = runtime
            .stream_turn(
                TurnInput {
                    items: turn_items,
                    image_blobs,
                    mode: None,
                    plan_file: None,
                },
                &sink,
                cancel.clone(),
            )
            .await
            .map_err(|e| format!("failed to run shepherd turn: {}", e))?;
        let mut turn = turn;
        let mut recovered_empty_output = false;
        let mut recovered_runtime_traceback = false;

        loop {
            info!(
                "shepherd lash turn complete: session={}, status={:?}, reason={:?}, output_state={:?}, safe_len={}, raw_len={}, errors={}",
                session_id,
                turn.status,
                turn.done_reason,
                turn.assistant_output.state,
                turn.assistant_output.safe_text.len(),
                turn.assistant_output.raw_text.len(),
                turn.errors.len()
            );

            let mut final_draft = draft.lock().await;
            let streamed_text = ShepherdLashSink::sanitize_assistant_text(final_draft.text.trim());
            let assembled_text =
                ShepherdLashSink::sanitize_assistant_text(&turn.assistant_output.safe_text);
            let runtime_output =
                ShepherdLashSink::sanitize_assistant_text(&final_draft.runtime_output);
            let final_text = if !assembled_text.trim().is_empty() {
                assembled_text.trim().to_string()
            } else if !streamed_text.trim().is_empty() {
                streamed_text
            } else {
                runtime_output.trim().to_string()
            };

            if !final_text.is_empty() {
                app.emit(
                    "shepherd-event",
                    (
                        &session_id,
                        &ShepherdEvent::TextDelta {
                            session_id: session_id.clone(),
                            text: final_text.clone(),
                        },
                    ),
                )
                .map_err(|e| format!("failed to emit shepherd text delta: {}", e))?;
                final_draft.text = final_text.clone();
            }
            info!(
                "shepherd draft summary: session={}, text_len={}, final_len={}, errored={}",
                session_id,
                final_draft.text.len(),
                final_text.len(),
                final_draft.errored
            );

            if matches!(turn.status, TurnStatus::Failed)
                && final_text.is_empty()
                && final_draft.tools.is_empty()
            {
                let message = turn
                    .errors
                    .first()
                    .map(|issue| issue.message.clone())
                    .unwrap_or_else(|| "Shepherd turn failed".to_string());
                final_draft.errored = true;
                drop(final_draft);
                app.emit(
                    "shepherd-event",
                    (
                        &session_id,
                        &ShepherdEvent::Error {
                            session_id: session_id.clone(),
                            message,
                        },
                    ),
                )
                .map_err(|e| format!("failed to emit shepherd error event: {}", e))?;
                return Ok(());
            }

            if matches!(turn.status, TurnStatus::Interrupted) {
                drop(final_draft);
                return Ok(());
            }

            if !recovered_runtime_traceback
                && matches!(turn.status, TurnStatus::Completed)
                && (matches!(turn.assistant_output.state, OutputState::TracebackOnly)
                    || looks_like_runtime_traceback(&final_text))
            {
                warn!(
                    "shepherd runtime traceback surfaced as assistant text; running one recovery pass: session={}, output_state={:?}",
                    session_id,
                    turn.assistant_output.state
                );
                *final_draft = AssistantDraft::default();
                drop(final_draft);
                recovered_runtime_traceback = true;
                turn = runtime
                    .stream_turn(
                        TurnInput {
                            items: vec![InputItem::Text {
                                text: "The previous attempt surfaced an internal runtime traceback. Answer the user's most recent message directly in plain language with no code blocks, no repl execution, and no traceback text.".to_string(),
                            }],
                            image_blobs: HashMap::new(),
                            mode: None,
                            plan_file: None,
                        },
                        &sink,
                        cancel.clone(),
                    )
                    .await
                    .map_err(|e| format!("failed to run shepherd traceback recovery turn: {}", e))?;
                continue;
            }

            if !recovered_empty_output
                && matches!(turn.status, TurnStatus::Completed)
                && final_text.is_empty()
                && final_draft.tools.is_empty()
            {
                warn!(
                    "shepherd empty output on completed turn; running one recovery pass: session={}, output_state={:?}, raw_assistant_output={:?}, runtime_output={:?}",
                    session_id,
                    turn.assistant_output.state,
                    turn.assistant_output.raw_text,
                    final_draft.runtime_output
                );
                *final_draft = AssistantDraft::default();
                drop(final_draft);
                recovered_empty_output = true;
                turn = runtime
                    .stream_turn(
                        TurnInput {
                            items: vec![InputItem::Text {
                                text: "Respond directly to the user's most recent message in plain language. Provide a complete, non-empty answer.".to_string(),
                            }],
                            image_blobs: HashMap::new(),
                            mode: None,
                            plan_file: None,
                        },
                        &sink,
                        cancel.clone(),
                    )
                    .await
                    .map_err(|e| format!("failed to run shepherd recovery turn: {}", e))?;
                continue;
            }

            if final_text.is_empty() && final_draft.tools.is_empty() {
                warn!(
                    "shepherd empty sanitized output: session={}, output_state={:?}, raw_assistant_output={:?}, runtime_output={:?}",
                    session_id,
                    turn.assistant_output.state,
                    turn.assistant_output.raw_text,
                    final_draft.runtime_output
                );
                let message = "Shepherd returned no user-visible output for this turn.".to_string();
                drop(final_draft);
                app.emit(
                    "shepherd-event",
                    (
                        &session_id,
                        &ShepherdEvent::Error {
                            session_id: session_id.clone(),
                            message,
                        },
                    ),
                )
                .map_err(|e| format!("failed to emit shepherd error event: {}", e))?;
                return Ok(());
            }

            if final_draft.errored && final_text.is_empty() && final_draft.tools.is_empty() {
                return Ok(());
            }

            let assistant_chunks = build_assistant_chunks(&final_draft, &final_text);
            if !assistant_chunks.is_empty() {
                let assistant_chunks_json = chunks_to_json(&assistant_chunks)?;
                save_message(&scope, "assistant", &assistant_chunks_json).await?;
            }

            drop(final_draft);
            break;
        }

        app.emit(
            "shepherd-event",
            (
                &session_id,
                &ShepherdEvent::MessageComplete {
                    session_id: session_id.clone(),
                },
            ),
        )
        .map_err(|e| format!("failed to emit shepherd complete event: {}", e))?;

        Ok(())
    }
    .await;

    if let Ok(mut guard) = sessions().lock() {
        if let Some(session) = guard.get_mut(&session_id) {
            session.active_turn = None;
            if let Some(runtime) = session_runtime.take() {
                session.runtime = Some(runtime);
            }
        }
    }

    result
}

/// Stop an active Shepherd session.
#[tauri::command]
pub async fn stop_shepherd_session(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<(), String> {
    let cancelled = sessions()
        .lock()
        .map_err(|_| "failed to lock Shepherd session map".to_string())?
        .remove(&session_id)
        .and_then(|s| s.active_turn);

    if let Some(cancel) = cancelled {
        cancel.cancel();
    }

    let ended = ShepherdEvent::SessionEnded {
        session_id: session_id.clone(),
    };
    app.emit("shepherd-event", (&session_id, &ended))
        .map_err(|e| format!("failed to emit shepherd session end event: {}", e))?;
    Ok(())
}

/// List active Shepherd sessions.
#[tauri::command]
pub async fn list_shepherd_sessions() -> Result<Vec<String>, String> {
    let guard = sessions()
        .lock()
        .map_err(|_| "failed to lock Shepherd session map".to_string())?;
    Ok(guard.keys().cloned().collect())
}

/// Get Shepherd chat history for the requested scope.
#[tauri::command]
pub async fn get_shepherd_history(
    scope: ShepherdScope,
    limit: usize,
) -> Result<Vec<crate::core::ShepherdChatMessage>, String> {
    let messages = load_scope_messages(&scope, limit).await?;

    Ok(match scope {
        ShepherdScope::Board { .. } => messages,
        _ => messages.into_iter().take(limit).collect(),
    })
}

/// Clear Shepherd history for the requested scope.
#[tauri::command]
pub async fn clear_shepherd_history(scope: ShepherdScope) -> Result<(), String> {
    let store = ShepherdChatStore::open().await.str_err()?;

    match scope {
        ShepherdScope::General => store.clear_messages(None).await.str_err()?,
        ShepherdScope::Run { run_name, .. } => {
            store.clear_messages(Some(&run_name)).await.str_err()?
        }
        ShepherdScope::Board { project_id, .. } => {
            store.clear_board_messages(project_id).await.str_err()?
        }
    }
    Ok(())
}

/// Save a Shepherd message chunk payload.
#[tauri::command]
pub async fn save_shepherd_message(
    scope: ShepherdScope,
    role: String,
    chunks_json: String,
) -> Result<i64, String> {
    let chunks: Vec<ShepherdMessageChunk> =
        serde_json::from_str(&chunks_json).map_err(|e| format!("invalid chunk payload: {}", e))?;
    let normalized = chunks_to_json(&chunks)?;
    save_message(&scope, &role, &normalized).await
}
