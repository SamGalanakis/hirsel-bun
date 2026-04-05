use std::path::{Path, PathBuf};

use crate::backend::project::ProjectStore;
use crate::backend::text_patch::TEXT_PATCH_INSTRUCTIONS;
use crate::backend::tool_results::edit_result_with;
use crate::backend::{ShepherdChatMessage, ShepherdThread, ShepherdThreadStore};
use lash::{ToolDefinition, ToolParam, ToolProvider, ToolResult};
use serde_json::{json, Map, Value};
use walkdir::WalkDir;

use super::commands::{
    archive_thread, close_thread_port_forward, create_thread, delete_thread, forward_thread_port,
    list_thread_port_forwards, promote_thread, send_scope_message,
};
use super::queries::{get_thread_activity, get_thread_conversation};
use super::rpc::{send_server_control_request, ServerControlRequest, ServerToolResultPayload};
use super::types::{ShepherdMessageChunk, ShepherdScope};

const NODE_READ_DEFAULT_LIMIT: usize = 2000;
const NODE_READ_MAX_LINE_LEN: usize = 2000;

#[cfg(feature = "gui")]
pub(super) type DesktopAppHandle = tauri::AppHandle;

#[cfg(not(feature = "gui"))]
#[derive(Clone, Debug)]
pub(super) struct DesktopAppHandle;

#[cfg(feature = "gui")]
fn emit_app_event(app: &DesktopAppHandle, event: &str, payload: Value) {
    let _ = tauri::Emitter::emit(app, event, payload);
}

#[cfg(not(feature = "gui"))]
fn emit_app_event(_app: &DesktopAppHandle, _event: &str, _payload: Value) {}

fn truncate_copy(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

macro_rules! tool_definition {
    ($($field:tt)*) => {
        ToolDefinition {
            $($field)*
            input_schema_override: None,
            output_schema_override: None,
        }
    };
}

struct ToolContext {
    app: Option<DesktopAppHandle>,
    default_project_id: Option<i64>,
    workspace_root: Option<PathBuf>,
}

impl ToolContext {
    fn new(
        app: Option<DesktopAppHandle>,
        default_project_id: Option<i64>,
        workspace_root: Option<PathBuf>,
    ) -> Self {
        Self {
            app,
            default_project_id,
            workspace_root,
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
            .ok_or_else(|| "project_id is required outside the shepherd session".to_string())
    }

    fn string_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
        args.get(key)
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("{} is required", key))
    }

    fn trimmed_string<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
        args.get(key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
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
                    return Err(ToolResult::err_fmt(
                        "Invalid limit: expected int, null, or \"none\"",
                    ));
                }
                let n = match v.as_u64() {
                    Some(n) => n,
                    None => {
                        return Err(ToolResult::err_fmt(
                            "Invalid limit: expected int, null, or \"none\"",
                        ));
                    }
                };
                if n == 0 {
                    return Err(ToolResult::err_fmt(
                        "Invalid limit: must be >= 1, or use null/\"none\" for no cap",
                    ));
                }
                Ok(Some(n as usize))
            }
        }
    }

    fn workspace_root(&self) -> Result<&Path, ToolResult> {
        self.workspace_root
            .as_deref()
            .ok_or_else(|| ToolResult::err_fmt("This scope has no attached workspace"))
    }

    fn resolve_workspace_path(&self, relative: Option<&str>) -> Result<PathBuf, ToolResult> {
        let root = self.workspace_root()?;
        let mut candidate = root.to_path_buf();
        if let Some(value) = relative.map(str::trim).filter(|value| !value.is_empty()) {
            let rel = Path::new(value);
            if rel.is_absolute() {
                return Err(ToolResult::err_fmt(
                    "Workspace paths must be relative to the attached workspace",
                ));
            }
            for component in rel.components() {
                if matches!(component, std::path::Component::ParentDir) {
                    return Err(ToolResult::err_fmt(
                        "Workspace paths may not escape the attached workspace",
                    ));
                }
            }
            candidate = root.join(rel);
        }
        Ok(candidate)
    }

    async fn proxy_shepherd_tool(&self, project_id: i64, name: &str, args: &Value) -> ToolResult {
        decode_server_tool_result(
            send_server_control_request(&ServerControlRequest::ExecuteShepherdTool {
                project_id,
                name: name.to_string(),
                args: args.clone(),
            })
            .await,
        )
    }

    async fn proxy_librarian_tool(&self, project_id: i64, name: &str, args: &Value) -> ToolResult {
        decode_server_tool_result(
            send_server_control_request(&ServerControlRequest::ExecuteLibrarianTool {
                project_id,
                name: name.to_string(),
                args: args.clone(),
            })
            .await,
        )
    }

    async fn list_workspace(&self, args: &Value) -> ToolResult {
        let target = match self.resolve_workspace_path(Self::trimmed_string(args, "path")) {
            Ok(path) => path,
            Err(error) => return error,
        };
        let max_depth = args
            .get("max_depth")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or(2);

        if !target.exists() {
            return ToolResult::err(json!({
                "error": format!("Workspace path does not exist: {}", target.display())
            }));
        }

        let root = match self.workspace_root() {
            Ok(root) => root,
            Err(error) => return error,
        };

        let entries = WalkDir::new(&target)
            .max_depth(max_depth)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path() != target)
            .filter_map(|entry| {
                let rel = entry.path().strip_prefix(root).ok()?;
                Some(json!({
                    "path": rel.display().to_string(),
                    "is_dir": entry.file_type().is_dir(),
                }))
            })
            .collect::<Vec<_>>();

        ToolResult::ok(json!({
            "root": root.display().to_string(),
            "entries": entries,
        }))
    }

    async fn read_workspace_file(&self, args: &Value) -> ToolResult {
        let relative = match Self::trimmed_string(args, "path") {
            Some(value) => value,
            None => return ToolResult::err_fmt("Missing required parameter: path"),
        };
        let target = match self.resolve_workspace_path(Some(relative)) {
            Ok(path) => path,
            Err(error) => return error,
        };
        if !target.exists() || !target.is_file() {
            return ToolResult::err(json!({
                "error": format!("Workspace file not found: {}", target.display())
            }));
        }

        let content = match std::fs::read_to_string(&target) {
            Ok(content) => content,
            Err(error) => {
                return ToolResult::err(json!({
                    "error": format!("Failed to read workspace file '{}': {}", target.display(), error)
                }));
            }
        };

        let offset = Self::parse_offset(args);
        let limit = match Self::parse_limit(args) {
            Ok(limit) => limit,
            Err(error) => return error,
        };
        let root = match self.workspace_root() {
            Ok(root) => root,
            Err(error) => return error,
        };
        let rel = target
            .strip_prefix(root)
            .unwrap_or(&target)
            .display()
            .to_string();

        let total_lines = content.lines().count();
        let start = offset.saturating_sub(1);
        let text = match limit {
            Some(limit) => content
                .lines()
                .skip(start)
                .take(limit)
                .map(|line| {
                    if line.len() > NODE_READ_MAX_LINE_LEN {
                        format!("{}...", &line[..NODE_READ_MAX_LINE_LEN])
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            None => content,
        };

        ToolResult::ok(json!({
            "path": rel,
            "offset": offset,
            "total_lines": total_lines,
            "content": text,
        }))
    }

    async fn grep_workspace(&self, args: &Value) -> ToolResult {
        let pattern = match Self::trimmed_string(args, "pattern") {
            Some(value) => value.to_string(),
            None => return ToolResult::err_fmt("Missing required parameter: pattern"),
        };
        let base = match self.resolve_workspace_path(Self::trimmed_string(args, "path")) {
            Ok(path) => path,
            Err(error) => return error,
        };
        let root = match self.workspace_root() {
            Ok(root) => root,
            Err(error) => return error,
        };

        let regex = match regex::Regex::new(&pattern) {
            Ok(regex) => regex,
            Err(error) => {
                return ToolResult::err(json!({
                    "error": format!("Invalid regex '{}': {}", pattern, error)
                }));
            }
        };

        let mut matches = Vec::new();
        for entry in WalkDir::new(&base)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            for (idx, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    let rel = entry
                        .path()
                        .strip_prefix(root)
                        .unwrap_or(entry.path())
                        .display()
                        .to_string();
                    matches.push(json!({
                        "path": rel,
                        "line": idx + 1,
                        "text": line,
                    }));
                    if matches.len() >= 200 {
                        break;
                    }
                }
            }
            if matches.len() >= 200 {
                break;
            }
        }

        ToolResult::ok(json!({
            "pattern": pattern,
            "matches": matches,
        }))
    }

    fn normalize_thread_status(status: &str) -> Option<&'static str> {
        match status.trim().to_ascii_lowercase().as_str() {
            "running" | "active" => Some("running"),
            "waiting" | "paused" => Some("waiting"),
            "blocked" => Some("blocked"),
            "done" | "completed" => Some("done"),
            "failed" | "error" => Some("failed"),
            "draft" | "idle" => Some("draft"),
            _ => None,
        }
    }

    fn latest_thread_plan(messages: &[ShepherdChatMessage]) -> Option<Value> {
        for message in messages.iter().rev() {
            let Ok(chunks) =
                serde_json::from_str::<Vec<ShepherdMessageChunk>>(&message.chunks_json)
            else {
                continue;
            };
            for chunk in chunks.into_iter().rev() {
                let ShepherdMessageChunk::Tool { input, .. } = chunk else {
                    continue;
                };
                let Some(input) = input else {
                    continue;
                };
                let Ok(parsed) = serde_json::from_str::<Value>(&input) else {
                    continue;
                };
                if parsed
                    .get("plan")
                    .and_then(|value| value.as_array())
                    .is_some()
                {
                    return Some(parsed);
                }
            }
        }
        None
    }

    async fn thread_store(&self) -> Result<ShepherdThreadStore, ToolResult> {
        ShepherdThreadStore::open()
            .await
            .map_err(|error| ToolResult::err(json!({ "error": error.to_string() })))
    }

    async fn resolve_thread(
        &self,
        project_id: i64,
        args: &Value,
    ) -> Result<ShepherdThread, ToolResult> {
        let store = self.thread_store().await?;
        if let Some(thread_id) = Self::trimmed_string(args, "thread_id") {
            let thread = store
                .get_thread(thread_id)
                .await
                .map_err(|error| ToolResult::err(json!({ "error": error.to_string() })))?;
            if thread.project_id != project_id {
                return Err(ToolResult::err(json!({
                    "error": format!("Thread {} does not belong to project {}", thread_id, project_id)
                })));
            }
            return Ok(thread);
        }
        let Some(title) = Self::trimmed_string(args, "title") else {
            return Err(ToolResult::err_fmt("Missing required parameter: thread_id"));
        };
        match store.find_project_thread_by_title(project_id, title).await {
            Ok(Some(thread)) => Ok(thread),
            Ok(None) => Err(ToolResult::err(json!({
                "error": format!("Thread '{}' not found", title)
            }))),
            Err(error) => Err(ToolResult::err(json!({ "error": error.to_string() }))),
        }
    }

    async fn list_threads_tool(&self, project_id: i64) -> ToolResult {
        let store = match self.thread_store().await {
            Ok(store) => store,
            Err(error) => return error,
        };
        match store.list_project_threads(project_id).await {
            Ok(threads) => ToolResult::ok(json!({ "threads": threads })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn create_thread_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let title = match Self::trimmed_string(args, "title") {
            Some(title) => title,
            None => return ToolResult::err_fmt("Missing required parameter: title"),
        };
        let objective = Self::trimmed_string(args, "objective").unwrap_or(title);
        let store = match self.thread_store().await {
            Ok(store) => store,
            Err(error) => return error,
        };
        let mut thread = match create_thread(project_id, title, objective).await {
            Ok(thread) => thread,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };

        if let Some(status) =
            Self::trimmed_string(args, "status").and_then(Self::normalize_thread_status)
        {
            let _ = store
                .update_thread(&thread.id, None, None, None, Some(status), None, None)
                .await;
            if let Ok(updated) = store.get_thread(&thread.id).await {
                thread = updated;
            }
        }

        ToolResult::ok(json!({
            "thread": thread,
            "message": format!("Created thread '{}'.", title),
        }))
    }

    async fn rename_thread_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let thread = match self.resolve_thread(project_id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        let new_title = match Self::trimmed_string(args, "new_title") {
            Some(title) => title,
            None => return ToolResult::err_fmt("Missing required parameter: new_title"),
        };
        let store = match self.thread_store().await {
            Ok(store) => store,
            Err(error) => return error,
        };
        match store
            .update_thread(&thread.id, Some(new_title), None, None, None, None, None)
            .await
        {
            Ok(()) => match store.get_thread(&thread.id).await {
                Ok(updated) => ToolResult::ok(json!({ "thread": updated })),
                Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
            },
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn set_thread_status_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let thread = match self.resolve_thread(project_id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        let status =
            match Self::trimmed_string(args, "status").and_then(Self::normalize_thread_status) {
                Some(status) => status,
                None => return ToolResult::err_fmt("Missing or invalid parameter: status"),
            };
        let summary = Self::trimmed_string(args, "summary");
        let store = match self.thread_store().await {
            Ok(store) => store,
            Err(error) => return error,
        };
        match store
            .update_thread(&thread.id, None, None, summary, Some(status), None, None)
            .await
        {
            Ok(()) => match store.get_thread(&thread.id).await {
                Ok(updated) => ToolResult::ok(json!({ "thread": updated })),
                Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
            },
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn archive_thread_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let thread = match self.resolve_thread(project_id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        match archive_thread(project_id, &thread.id).await {
            Ok(()) => ToolResult::ok(json!({ "success": true, "thread_id": thread.id })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn promote_thread_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let thread = match self.resolve_thread(project_id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        match promote_thread(project_id, &thread.id).await {
            Ok(result) => ToolResult::ok(json!({
                "thread": thread,
                "promotion": result,
                "message": if result.promoted {
                    format!("Promoted thread '{}' into central.", thread.title)
                } else {
                    format!("Thread '{}' had no new workspace changes to promote.", thread.title)
                },
            })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn delete_thread_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let thread = match self.resolve_thread(project_id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        match delete_thread(project_id, &thread.id).await {
            Ok(()) => ToolResult::ok(json!({ "success": true, "thread_id": thread.id })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn send_thread_message_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let thread = match self.resolve_thread(project_id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        let content = match Self::trimmed_string(args, "content") {
            Some(content) => content.to_string(),
            None => return ToolResult::err_fmt("Missing required parameter: content"),
        };
        if let Ok(store) = self.thread_store().await {
            let _ = store
                .update_thread(
                    &thread.id,
                    None,
                    None,
                    Some(&truncate_copy(&content, 180)),
                    Some("running"),
                    None,
                    None,
                )
                .await;
        }
        match send_scope_message(
            ShepherdScope::Thread {
                project_id,
                thread_id: thread.id.clone(),
                title: thread.title.clone(),
                workspace_path: None,
                focus: None,
            },
            Some(content),
            None,
            None,
        )
        .await
        {
            Ok(response) => ToolResult::ok(json!({
                "thread": thread,
                "dispatch": response,
            })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn read_thread_updates_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let thread = match self.resolve_thread(project_id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        let limit = Self::arg_i64(args, "limit").unwrap_or(24).max(1) as usize;
        let history =
            match get_thread_conversation(project_id, &thread.id, &thread.title, limit).await {
                Ok(history) => history,
                Err(error) => return ToolResult::err(json!({ "error": error })),
            };
        let activity = match get_thread_activity(project_id, &thread.id, &thread.title).await {
            Ok(activity) => activity,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };
        ToolResult::ok(json!({
            "thread": thread,
            "plan": Self::latest_thread_plan(&history),
            "history": history,
            "activity": activity,
        }))
    }

    async fn forward_port_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let thread = match self.resolve_thread(project_id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        let Some(port) = Self::arg_i64(args, "port") else {
            return ToolResult::err_fmt("Missing required parameter: port");
        };
        if !(1..=65535).contains(&port) {
            return ToolResult::err_fmt("Invalid parameter: port must be between 1 and 65535");
        }
        let protocol = Self::trimmed_string(args, "protocol")
            .unwrap_or("http")
            .to_ascii_lowercase();
        let Some(label) = Self::trimmed_string(args, "label") else {
            return ToolResult::err_fmt("Missing required parameter: label");
        };
        match forward_thread_port(project_id, &thread.id, port as u16, &protocol, label).await {
            Ok(forward) => ToolResult::ok(json!({
                "thread": thread,
                "forward": forward,
            })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn list_port_forwards_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let thread_id = if args.get("thread_id").is_some() || args.get("title").is_some() {
            match self.resolve_thread(project_id, args).await {
                Ok(thread) => Some(thread.id),
                Err(error) => return error,
            }
        } else {
            None
        };
        match list_thread_port_forwards(project_id, thread_id.as_deref()).await {
            Ok(forwards) => ToolResult::ok(json!({ "forwards": forwards })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn close_port_forward_tool(&self, _project_id: i64, args: &Value) -> ToolResult {
        let Some(forward_id) = Self::trimmed_string(args, "forward_id") else {
            return ToolResult::err_fmt("Missing required parameter: forward_id");
        };
        match close_thread_port_forward(forward_id).await {
            Ok(Some(forward)) => ToolResult::ok(json!({
                "closed": true,
                "forward": forward,
            })),
            Ok(None) => ToolResult::ok(json!({
                "closed": false,
                "forward": null,
            })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn read_project_focus_view(&self, project_id: i64) -> ToolResult {
        let store = match ProjectStore::open().await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        match store.get_project_focus_view(project_id).await {
            Ok(Some(view)) => ToolResult::ok(json!({
                "project_id": view.project_id,
                "html": view.html,
                "updated_at": view.updated_at,
                "source": view.source,
            })),
            Ok(None) => ToolResult::ok(json!({
                "project_id": project_id,
                "html": null,
                "updated_at": null,
                "source": null,
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn update_project_focus_view(&self, project_id: i64, args: &Value) -> ToolResult {
        let html = match Self::string_arg(args, "html") {
            Ok(value) => value,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };
        let source = args
            .get("source")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty());

        if html.trim().is_empty() {
            return ToolResult::err(json!({ "error": "Project canvas HTML cannot be empty" }));
        }

        let store = match ProjectStore::open().await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        match store
            .update_project_focus_view(project_id, html, source)
            .await
        {
            Ok(view) => {
                if let Some(app) = &self.app {
                    emit_app_event(
                        app,
                        "project-focus-view-updated",
                        json!({
                            "projectId": project_id,
                            "updatedAt": view.updated_at,
                        }),
                    );
                }
                let mut fields = Map::new();
                fields.insert("updated_at".to_string(), json!(view.updated_at));
                fields.insert("source".to_string(), json!(view.source));
                edit_result_with(
                    format!("Updated project canvas for project {}", project_id),
                    fields,
                )
            }
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn read_project_retained_context(&self, project_id: i64) -> ToolResult {
        let store = match ProjectStore::open().await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        match store.get_project_retained_context(project_id).await {
            Ok(context) => ToolResult::ok(json!({
                "project_id": context.project_id,
                "markdown": context.markdown,
                "updated_at": context.updated_at,
                "source": context.source,
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn update_project_retained_context(&self, project_id: i64, args: &Value) -> ToolResult {
        let markdown = match Self::string_arg(args, "markdown") {
            Ok(value) => value,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };
        let source = args
            .get("source")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty());

        let store = match ProjectStore::open().await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        match store
            .update_project_retained_context(project_id, markdown, source)
            .await
        {
            Ok(context) => {
                if let Some(app) = &self.app {
                    emit_app_event(
                        app,
                        "project-retained-context-updated",
                        json!({
                            "projectId": project_id,
                            "updatedAt": context.updated_at,
                        }),
                    );
                }
                let mut fields = Map::new();
                fields.insert("updated_at".to_string(), json!(context.updated_at));
                fields.insert("source".to_string(), json!(context.source));
                edit_result_with(
                    format!("Updated retained context for project {}", project_id),
                    fields,
                )
            }
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }
    async fn emit_knowledge_event(&self, project_id: i64, args: &Value) -> ToolResult {
        let kind = match Self::trimmed_string(args, "kind") {
            Some(kind) => kind,
            None => return ToolResult::err(json!({ "error": "kind is required" })),
        };
        let summary = match Self::trimmed_string(args, "summary") {
            Some(summary) => summary,
            None => return ToolResult::err(json!({ "error": "summary is required" })),
        };
        let files: Vec<String> = args
            .get("files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        // Grab the last 10 messages from shepherd chat for context
        let shepherd_scope = ShepherdScope::Shepherd {
            project_id,
            workspace_path: None,
            focus: None,
        };
        let conversation_tail = match super::queries::get_shepherd_history(shepherd_scope, 10).await
        {
            Ok(messages) => messages
                .into_iter()
                .map(|m| {
                    json!({
                        "role": m.role,
                        "chunks_json": m.chunks_json,
                    })
                })
                .collect::<Vec<_>>(),
            Err(_) => vec![],
        };

        // Enqueue the event for the Librarian
        let event = json!({
            "project_id": project_id,
            "kind": kind,
            "summary": summary,
            "files": files,
            "conversation_tail": conversation_tail,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "processed": false,
        });

        match crate::backend::librarian::enqueue_event(project_id, event).await {
            Ok(_) => ToolResult::ok(json!({
                "status": "queued",
                "kind": kind,
                "summary": summary,
            })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }
}

fn should_proxy_server_managed_tools() -> bool {
    std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok()
}

fn encode_tool_result_error(error: String) -> ToolResult {
    ToolResult::err(json!({ "error": error }))
}

fn decode_server_tool_result(payload: Result<Option<Value>, String>) -> ToolResult {
    let payload = match payload {
        Ok(Some(payload)) => payload,
        Ok(None) => {
            return encode_tool_result_error(
                "server control reply did not include a tool result payload".to_string(),
            );
        }
        Err(error) => return encode_tool_result_error(error),
    };

    let payload: ServerToolResultPayload = match serde_json::from_value(payload) {
        Ok(payload) => payload,
        Err(error) => {
            return encode_tool_result_error(format!(
                "failed to decode tool result payload: {}",
                error
            ));
        }
    };

    ToolResult {
        success: payload.success,
        result: payload.result,
        images: vec![],
    }
}

async fn execute_librarian_tool(
    common: &ToolContext,
    project_id: i64,
    name: &str,
    args: &Value,
) -> ToolResult {
    match name {
        "ls" => common.list_workspace(args).await,
        "read_file" => common.read_workspace_file(args).await,
        "grep" => common.grep_workspace(args).await,
        "graph_surql" => crate::backend::librarian::graph_surql(project_id, args).await,
        "edit_graph_node_text" => {
            crate::backend::librarian::edit_graph_node_text(project_id, args).await
        }
        _ => ToolResult::err(json!({ "error": format!("Unknown tool: {}", name) })),
    }
}

async fn execute_shepherd_tool(
    common: &ToolContext,
    project_id: i64,
    name: &str,
    args: &Value,
) -> ToolResult {
    match name {
        "list_threads" => common.list_threads_tool(project_id).await,
        "create_thread" => common.create_thread_tool(project_id, args).await,
        "rename_thread" => common.rename_thread_tool(project_id, args).await,
        "set_thread_status" => common.set_thread_status_tool(project_id, args).await,
        "archive_thread" => common.archive_thread_tool(project_id, args).await,
        "promote_thread" => common.promote_thread_tool(project_id, args).await,
        "delete_thread" => common.delete_thread_tool(project_id, args).await,
        "send_thread_message" => common.send_thread_message_tool(project_id, args).await,
        "read_thread_updates" => common.read_thread_updates_tool(project_id, args).await,
        "forward_port" => common.forward_port_tool(project_id, args).await,
        "list_port_forwards" => common.list_port_forwards_tool(project_id, args).await,
        "close_port_forward" => common.close_port_forward_tool(project_id, args).await,
        "read_project_focus_view" | "read_canvas" => {
            common.read_project_focus_view(project_id).await
        }
        "update_project_focus_view" | "update_canvas" => {
            common.update_project_focus_view(project_id, args).await
        }
        "read_project_retained_context" => common.read_project_retained_context(project_id).await,
        "update_project_retained_context" => {
            common
                .update_project_retained_context(project_id, args)
                .await
        }
        "ls" => common.list_workspace(args).await,
        "read_file" => common.read_workspace_file(args).await,
        "grep" => common.grep_workspace(args).await,
        "emit_knowledge_event" => common.emit_knowledge_event(project_id, args).await,
        _ => ToolResult::err(json!({ "error": format!("Unknown tool: {}", name) })),
    }
}

pub(crate) async fn execute_librarian_server_tool_local(
    project_id: i64,
    name: &str,
    args: &Value,
) -> ToolResult {
    let common = ToolContext::new(None, Some(project_id), None);
    execute_librarian_tool(&common, project_id, name, args).await
}

pub(crate) async fn execute_shepherd_server_tool_local(
    project_id: i64,
    name: &str,
    args: &Value,
) -> ToolResult {
    let common = ToolContext::new(None, Some(project_id), None);
    execute_shepherd_tool(&common, project_id, name, args).await
}

// ═══════════════════════════════════════
// Librarian Tool Provider
// ═══════════════════════════════════════

pub(super) struct LibrarianToolProvider {
    common: ToolContext,
}

impl LibrarianToolProvider {
    pub(super) fn new(
        app: Option<DesktopAppHandle>,
        default_project_id: Option<i64>,
        workspace_root: Option<PathBuf>,
    ) -> Self {
        Self {
            common: ToolContext::new(app, default_project_id, workspace_root),
        }
    }
}

#[async_trait::async_trait]
impl ToolProvider for LibrarianToolProvider {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![
            // Read-only workspace tools
            tool_definition! {
                name: "ls".to_string(),
                description: "List workspace directory contents.".to_string(),
                params: vec![
                    ToolParam::optional("path", "str"),
                    ToolParam::optional("depth", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "read_file".to_string(),
                description: "Read a text file from the workspace.".to_string(),
                params: vec![
                    ToolParam::typed("path", "str"),
                    ToolParam::optional("offset", "int"),
                    ToolParam::optional("limit", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "grep".to_string(),
                description: "Search workspace files with a regex pattern.".to_string(),
                params: vec![
                    ToolParam::typed("pattern", "str"),
                    ToolParam::optional("path", "str"),
                    ToolParam::optional("include", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "graph_surql".to_string(),
                description: "Run project-scoped SurrealQL against the knowledge graph. The backend binds `$project_id` automatically and only allows `kg_node` / `kg_edge` access.".to_string(),
                params: vec![
                    ToolParam::typed("query", "str"),
                    ToolParam::optional("params", "dict"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "edit_graph_node_text".to_string(),
                description: format!(
                    "Patch one top-level text field on an existing knowledge-graph node. Editable fields: summary, description, detail, notes, rationale, markdown.\n\n{}",
                    TEXT_PATCH_INSTRUCTIONS
                ),
                params: vec![
                    ToolParam::typed("kind", "str"),
                    ToolParam::typed("id", "str"),
                    ToolParam::typed("field", "str"),
                    ToolParam::typed("patch", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
        ]
    }

    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        let project_id = match self.common.resolve_project_id(args) {
            Ok(id) => id,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };

        if should_proxy_server_managed_tools() && !matches!(name, "ls" | "read_file" | "grep") {
            return self
                .common
                .proxy_librarian_tool(project_id, name, args)
                .await;
        }

        execute_librarian_tool(&self.common, project_id, name, args).await
    }
}

pub(super) struct ShepherdToolProvider {
    common: ToolContext,
}

impl ShepherdToolProvider {
    pub(super) fn new(
        app: Option<DesktopAppHandle>,
        default_project_id: Option<i64>,
        workspace_root: Option<PathBuf>,
    ) -> Self {
        Self {
            common: ToolContext::new(app, default_project_id, workspace_root),
        }
    }
}

#[async_trait::async_trait]
impl ToolProvider for ShepherdToolProvider {
    fn definitions(&self) -> Vec<ToolDefinition> {
        let mut definitions = vec![
            tool_definition! {
                name: "list_threads".to_string(),
                description: "List visible execution threads for the project.".to_string(),
                params: vec![ToolParam::optional("project_id", "int")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "create_thread".to_string(),
                description: "Create a new execution thread with its own container and workspace.".to_string(),
                params: vec![
                    ToolParam::typed("title", "str"),
                    ToolParam::optional("objective", "str"),
                    ToolParam::optional("status", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "rename_thread".to_string(),
                description: "Rename an existing thread.".to_string(),
                params: vec![
                    ToolParam::optional("thread_id", "str"),
                    ToolParam::optional("title", "str"),
                    ToolParam::typed("new_title", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "set_thread_status".to_string(),
                description: "Set thread status to running, waiting, blocked, done, failed, or draft.".to_string(),
                params: vec![
                    ToolParam::optional("thread_id", "str"),
                    ToolParam::optional("title", "str"),
                    ToolParam::typed("status", "str"),
                    ToolParam::optional("summary", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "archive_thread".to_string(),
                description: "Archive a thread so it drops out of the main thread deck.".to_string(),
                params: vec![
                    ToolParam::optional("thread_id", "str"),
                    ToolParam::optional("title", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delete_thread".to_string(),
                description: "Delete a thread and its transcript state.".to_string(),
                params: vec![
                    ToolParam::optional("thread_id", "str"),
                    ToolParam::optional("title", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "send_thread_message".to_string(),
                description: "Send a new message to a thread so it can continue or change direction.".to_string(),
                params: vec![
                    ToolParam::optional("thread_id", "str"),
                    ToolParam::optional("title", "str"),
                    ToolParam::typed("content", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "read_thread_updates".to_string(),
                description: "Read a thread transcript, live activity, and latest captured plan.".to_string(),
                params: vec![
                    ToolParam::optional("thread_id", "str"),
                    ToolParam::optional("title", "str"),
                    ToolParam::optional("limit", "int"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "forward_port".to_string(),
                description: "Expose an HTTP or HTTPS port from a thread container to the user and to shepherd. Labels are required.".to_string(),
                params: vec![
                    ToolParam::optional("thread_id", "str"),
                    ToolParam::optional("title", "str"),
                    ToolParam::typed("port", "int"),
                    ToolParam::optional("protocol", "str"),
                    ToolParam::typed("label", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "list_port_forwards".to_string(),
                description: "List active forwarded previews for the project or for one thread.".to_string(),
                params: vec![
                    ToolParam::optional("thread_id", "str"),
                    ToolParam::optional("title", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "close_port_forward".to_string(),
                description: "Close a previously opened forwarded preview.".to_string(),
                params: vec![ToolParam::typed("forward_id", "str")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
        ];

        definitions.push(tool_definition! {
            name: "promote_thread".to_string(),
            description: "Merge a completed thread checkout back into the central checkout.".to_string(),
            params: vec![
                ToolParam::optional("thread_id", "str"),
                ToolParam::optional("title", "str"),
                ToolParam::optional("project_id", "int"),
            ],
            returns: "dict".to_string(),
            examples: vec![],
            enabled: true,
            injected: true,
        });

        if self.common.workspace_root.is_some() {
            definitions.extend([
                tool_definition! {
                    name: "ls".to_string(),
                    description: "List files and directories inside the attached workspace. Paths must stay relative to that workspace.".to_string(),
                    params: vec![
                        ToolParam::optional("path", "str"),
                        ToolParam::optional("max_depth", "int"),
                        ToolParam::optional("project_id", "int"),
                    ],
                    returns: "dict".to_string(),
                    examples: vec![],
                    enabled: true,
                    injected: true,
                },
                tool_definition! {
                    name: "read_file".to_string(),
                    description: "Read a text file from the attached workspace. The path must be relative to that workspace.".to_string(),
                    params: vec![
                        ToolParam::typed("path", "str"),
                        ToolParam::optional("offset", "int"),
                        ToolParam::optional("limit", "int"),
                        ToolParam::optional("project_id", "int"),
                    ],
                    returns: "dict".to_string(),
                    examples: vec![],
                    enabled: true,
                    injected: true,
                },
                tool_definition! {
                    name: "grep".to_string(),
                    description: "Search text files inside the attached workspace using a regex pattern.".to_string(),
                    params: vec![
                        ToolParam::typed("pattern", "str"),
                        ToolParam::optional("path", "str"),
                        ToolParam::optional("project_id", "int"),
                    ],
                    returns: "dict".to_string(),
                    examples: vec![],
                    enabled: true,
                    injected: true,
                },
            ]);
        }

        definitions.extend([
            tool_definition! {
                name: "read_project_focus_view".to_string(),
                description: "Read the current project canvas HTML fragment for this project.".to_string(),
                params: vec![ToolParam::optional("project_id", "int")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "read_canvas".to_string(),
                description: "Read the current project canvas HTML fragment for this project.".to_string(),
                params: vec![ToolParam::optional("project_id", "int")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "update_project_focus_view".to_string(),
                description: "Replace the current project canvas HTML fragment for this project.".to_string(),
                params: vec![
                    ToolParam::typed("html", "str"),
                    ToolParam::optional("source", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "EditResult".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "update_canvas".to_string(),
                description: "Replace the current project canvas HTML fragment for this project.".to_string(),
                params: vec![
                    ToolParam::typed("html", "str"),
                    ToolParam::optional("source", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "EditResult".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "read_project_retained_context".to_string(),
                description: "Read the current project-level retained context markdown for this project.".to_string(),
                params: vec![ToolParam::optional("project_id", "int")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "update_project_retained_context".to_string(),
                description: "Replace the project-level retained context markdown for this project.".to_string(),
                params: vec![
                    ToolParam::typed("markdown", "str"),
                    ToolParam::optional("source", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "EditResult".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "emit_knowledge_event".to_string(),
                description: "Signal the Librarian to record a piece of project knowledge — an idea, decision, observation, risk, or bug discovered during conversation. The Librarian will process it asynchronously; this call returns immediately. Use liberally: if the user expresses intent, makes a decision, or you notice something noteworthy about the codebase, emit an event.".to_string(),
                params: vec![
                    ToolParam::typed("kind", "str"),
                    ToolParam::typed("summary", "str"),
                    ToolParam::optional("files", "list[str]"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
        ]);

        definitions
    }

    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        let project_id = match self.common.resolve_project_id(args) {
            Ok(project_id) => project_id,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };

        if should_proxy_server_managed_tools() && !matches!(name, "ls" | "read_file" | "grep") {
            return self
                .common
                .proxy_shepherd_tool(project_id, name, args)
                .await;
        }

        execute_shepherd_tool(&self.common, project_id, name, args).await
    }
}
