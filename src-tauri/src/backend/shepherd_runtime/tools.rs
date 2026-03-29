use std::path::{Path, PathBuf};

use crate::backend::app::{delivery, routes};
use crate::backend::db::{global_pool, utc_now};
use crate::backend::delta::DeltaState;
use crate::backend::project::{validate_project_focus_view_html, ProjectStore};
use crate::backend::route::{CreateRouteRequest, Route, RouteStore};
use crate::backend::shepherd_threads::prepare_thread_workspace;
use crate::backend::{ShepherdChatMessage, ShepherdThread, ShepherdThreadStore};
use lash::{ToolDefinition, ToolParam, ToolProvider, ToolResult};
use serde_json::{json, Value};
use walkdir::WalkDir;

use super::commands::{
    enqueue_shepherd_message_for_scope, get_thread_conversation, get_thread_queue,
};
use super::types::{ShepherdMessageChunk, ShepherdScope};

const NODE_READ_DEFAULT_LIMIT: usize = 2000;
const NODE_READ_MAX_LINE_LEN: usize = 2000;
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
    ($($field:tt)* input_schema_override: $input:expr, output_schema_override: $output:expr $(,)?) => {
        ToolDefinition {
            $($field)*
            input_schema_override: $input,
            output_schema_override: $output,
        }
    };
    ($($field:tt)*) => {
        ToolDefinition {
            $($field)*
            input_schema_override: None,
            output_schema_override: None,
        }
    };
}

pub(super) struct ShepherdToolProvider {
    app: Option<tauri::AppHandle>,
    default_project_id: Option<i64>,
    workspace_root: Option<PathBuf>,
}

impl ShepherdToolProvider {
    pub(super) fn new(
        app: Option<tauri::AppHandle>,
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
            .ok_or_else(|| "project_id is required outside project scope".to_string())
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

    fn route_param_defs() -> Vec<ToolParam> {
        vec![
            ToolParam::optional("project_id", "int"),
            ToolParam::optional("route_name", "str"),
        ]
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
                    "Workspace paths must be relative to the attached route workspace",
                ));
            }
            for component in rel.components() {
                if matches!(component, std::path::Component::ParentDir) {
                    return Err(ToolResult::err_fmt(
                        "Workspace paths may not escape the attached route workspace",
                    ));
                }
            }
            candidate = root.join(rel);
        }
        Ok(candidate)
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
                        format!("{}…", &line[..NODE_READ_MAX_LINE_LEN])
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

    async fn selected_route(&self, project_id: i64) -> Result<Route, ToolResult> {
        let project_store = match ProjectStore::open().await {
            Ok(store) => store,
            Err(error) => return Err(ToolResult::err(json!({ "error": error.to_string() }))),
        };
        let route_store = match RouteStore::new(project_id).await {
            Ok(store) => store,
            Err(error) => return Err(ToolResult::err(json!({ "error": error.to_string() }))),
        };

        let project = match project_store.get_project(project_id).await {
            Ok(project) => project,
            Err(error) => return Err(ToolResult::err(json!({ "error": error.to_string() }))),
        };

        if let Some(route_id) = project.active_route_id {
            if let Ok(route) = route_store.get_route(route_id).await {
                return Ok(route);
            }
        }

        match route_store.list_routes().await {
            Ok(routes) => routes.into_iter().next().ok_or_else(|| {
                ToolResult::err(json!({
                    "error": "No routes exist for this project"
                }))
            }),
            Err(error) => Err(ToolResult::err(json!({ "error": error.to_string() }))),
        }
    }

    async fn resolve_route(&self, project_id: i64, args: &Value) -> Result<Route, ToolResult> {
        let route_store = match RouteStore::new(project_id).await {
            Ok(store) => store,
            Err(error) => return Err(ToolResult::err(json!({ "error": error.to_string() }))),
        };

        if let Some(route_name) = Self::trimmed_string(args, "route_name") {
            return match route_store.get_route_by_name(route_name).await {
                Ok(Some(route)) => Ok(route),
                Ok(None) => Err(ToolResult::err(json!({
                    "error": format!("Route '{}' not found", route_name)
                }))),
                Err(error) => Err(ToolResult::err(json!({ "error": error.to_string() }))),
            };
        }

        if let Some(route_id) = Self::arg_i64(args, "route_id") {
            return match route_store.get_route(route_id).await {
                Ok(route) => Ok(route),
                Err(error) => Err(ToolResult::err(json!({ "error": error.to_string() }))),
            };
        }

        self.selected_route(project_id).await
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
        route_id: i64,
        args: &Value,
    ) -> Result<ShepherdThread, ToolResult> {
        let store = self.thread_store().await?;
        if let Some(thread_id) = Self::trimmed_string(args, "thread_id") {
            return store
                .get_thread(thread_id)
                .await
                .map_err(|error| ToolResult::err(json!({ "error": error.to_string() })));
        }
        let Some(title) = Self::trimmed_string(args, "title") else {
            return Err(ToolResult::err_fmt("Missing required parameter: thread_id"));
        };
        match store
            .find_route_thread_by_title(project_id, route_id, title)
            .await
        {
            Ok(Some(thread)) => Ok(thread),
            Ok(None) => Err(ToolResult::err(json!({
                "error": format!("Thread '{}' not found", title)
            }))),
            Err(error) => Err(ToolResult::err(json!({ "error": error.to_string() }))),
        }
    }

    async fn list_routes(&self, project_id: i64) -> ToolResult {
        let project_store = match ProjectStore::open().await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };
        let route_store = match RouteStore::new(project_id).await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        let project = match project_store.get_project(project_id).await {
            Ok(project) => project,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };
        let selected_route_id = project.active_route_id;

        match route_store.list_routes().await {
            Ok(routes) => {
                let routes = routes
                    .into_iter()
                    .map(|route| {
                        let mut value = json!({
                            "id": route.id,
                            "name": route.name,
                            "created_at": route.created_at,
                            "selected": selected_route_id == Some(route.id),
                        });
                        if let Some(parent_route_id) = route.parent_route_id {
                            value["parent_route_id"] = json!(parent_route_id);
                        }
                        if let Some(parent_version_id) = route.parent_version_id {
                            value["forked_from_version"] = json!(parent_version_id);
                        }
                        value
                    })
                    .collect::<Vec<_>>();

                ToolResult::ok(json!({
                    "routes": routes,
                    "selected_route_id": selected_route_id,
                }))
            }
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn list_threads_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let store = match self.thread_store().await {
            Ok(store) => store,
            Err(error) => return error,
        };
        match store.list_route_threads(project_id, route.id).await {
            Ok(threads) => ToolResult::ok(json!({
                "route": { "id": route.id, "name": route.name },
                "threads": threads,
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn create_thread_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let title = match Self::trimmed_string(args, "title") {
            Some(title) => title,
            None => return ToolResult::err_fmt("Missing required parameter: title"),
        };
        let objective = Self::trimmed_string(args, "objective").unwrap_or(title);
        let summary = truncate_copy(objective, 180);
        let (workspace_path, checkout_name) =
            match prepare_thread_workspace(project_id, route.id, title).await {
                Ok(result) => result,
                Err(error) => return ToolResult::err(json!({ "error": error })),
            };
        let store = match self.thread_store().await {
            Ok(store) => store,
            Err(error) => return error,
        };
        let mut thread = match store
            .create_thread(
                project_id,
                route.id,
                title,
                objective,
                &summary,
                Some(&workspace_path),
                Some(&checkout_name),
            )
            .await
        {
            Ok(thread) => thread,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
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
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let thread = match self.resolve_thread(project_id, route.id, args).await {
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
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let thread = match self.resolve_thread(project_id, route.id, args).await {
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
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let thread = match self.resolve_thread(project_id, route.id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        let store = match self.thread_store().await {
            Ok(store) => store,
            Err(error) => return error,
        };
        match store.archive_thread(&thread.id).await {
            Ok(()) => ToolResult::ok(json!({ "success": true, "thread_id": thread.id })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn delete_thread_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let thread = match self.resolve_thread(project_id, route.id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        let store = match self.thread_store().await {
            Ok(store) => store,
            Err(error) => return error,
        };
        if let Ok(chat_store) = crate::backend::ShepherdChatStore::open().await {
            let _ = chat_store
                .delete_run_messages(&ShepherdThreadStore::runtime_name(&thread.id))
                .await;
            let _ = chat_store
                .clear_scope_state(project_id, &ShepherdThreadStore::runtime_name(&thread.id))
                .await;
        }
        if let Some(workspace_path) = thread
            .workspace_path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
        {
            let workspace = PathBuf::from(workspace_path);
            if workspace.exists() {
                if let Err(error) = std::fs::remove_dir_all(&workspace) {
                    tracing::warn!(
                        %error,
                        thread_id = %thread.id,
                        workspace = %workspace.display(),
                        "failed to remove thread workspace"
                    );
                }
            }
        }
        match store.delete_thread(&thread.id).await {
            Ok(()) => ToolResult::ok(json!({ "success": true, "thread_id": thread.id })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn send_thread_message_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let thread = match self.resolve_thread(project_id, route.id, args).await {
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
        match enqueue_shepherd_message_for_scope(
            ShepherdScope::Thread {
                project_id,
                route_id: route.id,
                thread_id: thread.id.clone(),
                title: thread.title.clone(),
                workspace_path: thread.workspace_path.clone(),
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
                "queue": response,
            })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn read_thread_updates_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let thread = match self.resolve_thread(project_id, route.id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        let limit = Self::arg_i64(args, "limit").unwrap_or(24).max(1) as usize;
        let history =
            match get_thread_conversation(project_id, route.id, &thread.id, &thread.title, limit)
                .await
            {
                Ok(history) => history,
                Err(error) => return ToolResult::err(json!({ "error": error })),
            };
        let queue = match get_thread_queue(project_id, route.id, &thread.id, &thread.title).await {
            Ok(queue) => queue,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };
        ToolResult::ok(json!({
            "thread": thread,
            "plan": Self::latest_thread_plan(&history),
            "history": history,
            "queue": queue,
        }))
    }

    async fn create_route(&self, project_id: i64, args: &Value) -> ToolResult {
        let name = match Self::trimmed_string(args, "name") {
            Some(name) => name,
            None => return ToolResult::err_fmt("Missing required parameter: name"),
        };
        let route_store = match RouteStore::new(project_id).await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        let parent_route_id =
            if let Some(parent_name) = Self::trimmed_string(args, "parent_route_name") {
                match route_store.get_route_by_name(parent_name).await {
                    Ok(Some(route)) => Some(route.id),
                    Ok(None) => {
                        return ToolResult::err(json!({
                            "error": format!("Parent route '{}' not found", parent_name)
                        }));
                    }
                    Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
                }
            } else {
                Some(match self.selected_route(project_id).await {
                    Ok(route) => route.id,
                    Err(error) => return error,
                })
            };

        let req = CreateRouteRequest {
            name: name.to_string(),
            parent_route_id,
            parent_version_id: None,
        };

        match route_store.create_route(&req).await {
            Ok(route) => ToolResult::ok(json!({
                "success": true,
                "route": {
                    "id": route.id,
                    "name": route.name,
                    "parent_route_id": route.parent_route_id,
                },
                "message": format!("Created route '{}'.", route.name),
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn select_route(&self, project_id: i64, args: &Value) -> ToolResult {
        let name = match Self::trimmed_string(args, "name") {
            Some(name) => name,
            None => return ToolResult::err_fmt("Missing required parameter: name"),
        };

        let route_store = match RouteStore::new(project_id).await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };
        let route = match route_store.get_route_by_name(name).await {
            Ok(Some(route)) => route,
            Ok(None) => {
                return ToolResult::err(json!({
                    "error": format!("Route '{}' not found", name)
                }));
            }
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        let pool = global_pool().await;
        match sqlx::query("UPDATE projects SET active_route_id = ?, updated_at = ? WHERE id = ?")
            .bind(route.id)
            .bind(utc_now())
            .bind(project_id)
            .execute(pool)
            .await
        {
            Ok(_) => ToolResult::ok(json!({
                "success": true,
                "route": {
                    "id": route.id,
                    "name": route.name,
                },
                "message": format!("Selected route '{}'.", route.name),
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn archive_route(&self, project_id: i64, args: &Value) -> ToolResult {
        let name = match Self::trimmed_string(args, "name") {
            Some(name) => name,
            None => return ToolResult::err_fmt("Missing required parameter: name"),
        };

        let route_store = match RouteStore::new(project_id).await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };
        let route = match route_store.get_route_by_name(name).await {
            Ok(Some(route)) => route,
            Ok(None) => {
                return ToolResult::err(json!({
                    "error": format!("Route '{}' not found", name)
                }));
            }
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        match routes::archive_route(project_id, route.id).await {
            Ok(archived) => {
                let selected_route = routes::get_active_route(project_id).await.ok();
                ToolResult::ok(json!({
                    "success": true,
                    "archived_route": {
                        "id": archived.id,
                        "name": archived.name,
                    },
                    "selected_route": selected_route.map(|selected| json!({
                        "id": selected.id,
                        "name": selected.name,
                    })),
                    "message": format!("Archived route '{}'.", route.name),
                }))
            }
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn latest_version_id(&self, project_id: i64, route_id: i64) -> Result<i64, ToolResult> {
        match delivery::get_latest_board_version(project_id, route_id).await {
            Ok(Some(version)) => Ok(version.id),
            Ok(None) => Err(ToolResult::err(json!({
                "error": "No board version exists for this route yet"
            }))),
            Err(error) => Err(ToolResult::err(json!({ "error": error }))),
        }
    }

    async fn resolve_existing_delivery_id(
        &self,
        project_id: i64,
        route_id: i64,
        args: &Value,
    ) -> Result<i64, ToolResult> {
        if let Some(delivery_id) = Self::arg_i64(args, "delivery_id") {
            let state = DeltaState::with_route(project_id, route_id);
            return match state.get_delivery(delivery_id).await {
                Ok(delivery) => Ok(delivery.id),
                Err(error) => Err(ToolResult::err(json!({ "error": error.to_string() }))),
            };
        }

        match delivery::get_current_board_delivery(project_id, route_id).await {
            Ok(Some(current)) => Ok(current.id),
            Ok(None) => Err(ToolResult::err(json!({
                "error": "No active delivery exists for this route"
            }))),
            Err(error) => Err(ToolResult::err(json!({ "error": error }))),
        }
    }

    async fn start_delivery(&self, project_id: i64, route_id: i64, args: &Value) -> ToolResult {
        let target_branch = match Self::trimmed_string(args, "target_branch") {
            Some(branch) => branch.to_string(),
            None => return ToolResult::err_fmt("Missing required parameter: target_branch"),
        };
        let version_id = match Self::arg_i64(args, "version_id") {
            Some(version_id) => version_id,
            None => match self.latest_version_id(project_id, route_id).await {
                Ok(version_id) => version_id,
                Err(error) => return error,
            },
        };
        let remote_url = Self::trimmed_string(args, "remote_url").map(ToOwned::to_owned);

        match delivery::start_board_delivery(
            project_id,
            route_id,
            version_id,
            target_branch,
            false,
            remote_url,
        )
        .await
        {
            Ok(delivery) => ToolResult::ok(json!({ "delivery": delivery })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn publish_delivery(&self, project_id: i64, route_id: i64, args: &Value) -> ToolResult {
        let publish_as = Self::trimmed_string(args, "publish_as").unwrap_or("push");
        if !matches!(publish_as, "push" | "pr") {
            return ToolResult::err(json!({
                "error": "publish_as must be either 'push' or 'pr'"
            }));
        }

        let delivery_id = match self
            .resolve_existing_delivery_id(project_id, route_id, args)
            .await
        {
            Ok(delivery_id) => delivery_id,
            Err(_) => {
                let started = self.start_delivery(project_id, route_id, args).await;
                if !started.success {
                    return started;
                }
                match started
                    .result
                    .get("delivery")
                    .and_then(|value| value.get("id"))
                    .and_then(|value| value.as_i64())
                {
                    Some(delivery_id) => delivery_id,
                    None => {
                        return ToolResult::err(json!({
                            "error": "Failed to resolve delivery after starting it"
                        }));
                    }
                }
            }
        };
        let summary = Self::trimmed_string(args, "summary").map(ToOwned::to_owned);
        let remote_url = Self::trimmed_string(args, "remote_url").map(ToOwned::to_owned);

        match delivery::complete_board_delivery(
            project_id,
            route_id,
            delivery_id,
            publish_as.to_string(),
            summary,
            remote_url,
        )
        .await
        {
            Ok(delivery) => ToolResult::ok(json!({ "delivery": delivery })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn merge_delivery(&self, project_id: i64, route_id: i64, args: &Value) -> ToolResult {
        let delivery_id = match self
            .resolve_existing_delivery_id(project_id, route_id, args)
            .await
        {
            Ok(delivery_id) => delivery_id,
            Err(_) => {
                let started = self.start_delivery(project_id, route_id, args).await;
                if !started.success {
                    return started;
                }
                match started
                    .result
                    .get("delivery")
                    .and_then(|value| value.get("id"))
                    .and_then(|value| value.as_i64())
                {
                    Some(delivery_id) => delivery_id,
                    None => {
                        return ToolResult::err(json!({
                            "error": "Failed to resolve delivery after starting it"
                        }));
                    }
                }
            }
        };
        let summary = Self::trimmed_string(args, "summary").map(ToOwned::to_owned);
        let remote_url = Self::trimmed_string(args, "remote_url").map(ToOwned::to_owned);

        match delivery::complete_board_delivery(
            project_id,
            route_id,
            delivery_id,
            "merge".to_string(),
            summary,
            remote_url,
        )
        .await
        {
            Ok(delivery) => ToolResult::ok(json!({ "delivery": delivery })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn read_project_focus_view(&self, project_id: i64) -> ToolResult {
        let store = match ProjectStore::open().await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        match store.get_project_focus_view(project_id).await {
            Ok(view) => ToolResult::ok(json!({
                "project_id": view.project_id,
                "html": view.html,
                "updated_at": view.updated_at,
                "source": view.source,
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

        if let Err(error) = validate_project_focus_view_html(html) {
            return ToolResult::err(json!({ "error": error }));
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
                    let _ = tauri::Emitter::emit(
                        app,
                        "project-focus-view-updated",
                        json!({
                            "projectId": project_id,
                            "updatedAt": view.updated_at,
                        }),
                    );
                }
                ToolResult::ok(json!({
                    "__type__": "edit_result",
                    "summary": format!("Updated project focus view for project {}", project_id),
                    "updated_at": view.updated_at,
                    "source": view.source,
                }))
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
                    let _ = tauri::Emitter::emit(
                        app,
                        "project-retained-context-updated",
                        json!({
                            "projectId": project_id,
                            "updatedAt": context.updated_at,
                        }),
                    );
                }
                ToolResult::ok(json!({
                    "__type__": "edit_result",
                    "summary": format!("Updated retained context for project {}", project_id),
                    "updated_at": context.updated_at,
                    "source": context.source,
                }))
            }
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }
}

#[async_trait::async_trait]
impl ToolProvider for ShepherdToolProvider {
    fn definitions(&self) -> Vec<ToolDefinition> {
        let mut definitions = vec![
            tool_definition! {
                name: "list_routes".to_string(),
                description: "List active routes for the project and show which route is currently selected.".to_string(),
                params: vec![ToolParam::optional("project_id", "int")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "fork_route".to_string(),
                description: "Fork a new route by name. Uses parent_route_name when provided; otherwise forks from the selected route.".to_string(),
                params: vec![
                    ToolParam::typed("name", "str"),
                    ToolParam::optional("parent_route_name", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "select_route".to_string(),
                description: "Select the project's route by unique name. Route-scoped tools default to this route when route_name is omitted.".to_string(),
                params: vec![
                    ToolParam::typed("name", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "archive_route".to_string(),
                description: "Archive a route by unique name. Archived routes drop out of normal active route flows.".to_string(),
                params: vec![
                    ToolParam::typed("name", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "list_threads".to_string(),
                description: "List visible execution threads for a route.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "create_thread".to_string(),
                description: "Create a new execution thread with its own checkout. Use this when parallel work should become a visible durable thread.".to_string(),
                params: vec![
                    ToolParam::typed("title", "str"),
                    ToolParam::optional("objective", "str"),
                    ToolParam::optional("status", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
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
                    ToolParam::optional("route_name", "str"),
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
                    ToolParam::optional("route_name", "str"),
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
                    ToolParam::optional("route_name", "str"),
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
                    ToolParam::optional("route_name", "str"),
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
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "read_thread_updates".to_string(),
                description: "Read a thread transcript, queue state, and latest captured plan.".to_string(),
                params: vec![
                    ToolParam::optional("thread_id", "str"),
                    ToolParam::optional("title", "str"),
                    ToolParam::optional("limit", "int"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
        ];

        if self.workspace_root.is_some() {
            definitions.extend([
                tool_definition! {
                    name: "list_workspace".to_string(),
                    description: "List files and directories inside the attached route workspace. Paths must stay relative to that workspace.".to_string(),
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
                    name: "read_workspace_file".to_string(),
                    description: "Read a text file from the attached route workspace. The path must be relative to that workspace.".to_string(),
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
                    name: "grep_workspace".to_string(),
                    description: "Search text files inside the attached route workspace using a regex pattern.".to_string(),
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
                name: "delivery_validate_target".to_string(),
                description: "Validate whether a route can be delivered to a target branch. Uses the selected route when route_name is omitted.".to_string(),
                params: vec![
                    ToolParam::typed("target_branch", "str"),
                    ToolParam::optional("remote_url", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delivery_get_versions".to_string(),
                description: "List published board versions available for delivery on a route. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delivery_get_latest_version".to_string(),
                description: "Get the latest board version for a route. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delivery_get_current".to_string(),
                description: "Get the current non-terminal delivery for a route. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delivery_start".to_string(),
                description: "Start a new delivery for a route. Uses the latest board version when version_id is omitted.".to_string(),
                params: vec![
                    ToolParam::typed("target_branch", "str"),
                    ToolParam::optional("version_id", "int"),
                    ToolParam::optional("remote_url", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delivery_publish".to_string(),
                description: "Publish a route delivery by pushing a branch or opening a PR. Reuses the current delivery when available, or starts one if needed.".to_string(),
                params: vec![
                    ToolParam::optional("delivery_id", "int"),
                    ToolParam::optional("target_branch", "str"),
                    ToolParam::optional("version_id", "int"),
                    ToolParam::optional("publish_as", "str"),
                    ToolParam::optional("summary", "str"),
                    ToolParam::optional("remote_url", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delivery_merge".to_string(),
                description: "Merge a route delivery. Reuses the current delivery when available, or starts one if needed.".to_string(),
                params: vec![
                    ToolParam::optional("delivery_id", "int"),
                    ToolParam::optional("target_branch", "str"),
                    ToolParam::optional("version_id", "int"),
                    ToolParam::optional("summary", "str"),
                    ToolParam::optional("remote_url", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delivery_get_attempts".to_string(),
                description: "Get retry history for a delivery on a route.".to_string(),
                params: vec![
                    ToolParam::typed("delivery_id", "int"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delivery_retry".to_string(),
                description: "Retry a failed delivery on a route.".to_string(),
                params: vec![
                    ToolParam::typed("delivery_id", "int"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delivery_complete".to_string(),
                description: "Complete an existing delivery using action 'push', 'pr', or 'merge'.".to_string(),
                params: vec![
                    ToolParam::typed("delivery_id", "int"),
                    ToolParam::typed("action", "str"),
                    ToolParam::optional("summary", "str"),
                    ToolParam::optional("remote_url", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delivery_abandon".to_string(),
                description: "Abandon the current or specified delivery on a route.".to_string(),
                params: vec![
                    ToolParam::optional("delivery_id", "int"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "read_project_focus_view".to_string(),
                description: "Read the current project-focus HTML artifact for this project.".to_string(),
                params: vec![ToolParam::optional("project_id", "int")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "read_canvas".to_string(),
                description: "Read the current canvas HTML artifact for this project.".to_string(),
                params: vec![ToolParam::optional("project_id", "int")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "update_project_focus_view".to_string(),
                description: "Replace the project-focus HTML artifact for this project with a full HTML document. Inline Mermaid setup is allowed.".to_string(),
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
                description: "Replace the current canvas HTML artifact for this project with a full HTML document.".to_string(),
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
        ]);

        definitions
    }

    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        let project_id = match self.resolve_project_id(args) {
            Ok(project_id) => project_id,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };

        match name {
            "list_routes" => self.list_routes(project_id).await,
            "fork_route" => self.create_route(project_id, args).await,
            "select_route" => self.select_route(project_id, args).await,
            "archive_route" => self.archive_route(project_id, args).await,
            "read_project_focus_view" | "read_canvas" => {
                self.read_project_focus_view(project_id).await
            }
            "update_project_focus_view" | "update_canvas" => {
                self.update_project_focus_view(project_id, args).await
            }
            "read_project_retained_context" => self.read_project_retained_context(project_id).await,
            "update_project_retained_context" => {
                self.update_project_retained_context(project_id, args).await
            }
            "list_workspace" => self.list_workspace(args).await,
            "read_workspace_file" => self.read_workspace_file(args).await,
            "grep_workspace" => self.grep_workspace(args).await,
            _ => {
                let route = match self.resolve_route(project_id, args).await {
                    Ok(route) => route,
                    Err(error) => return error,
                };
                let route_id = route.id;

                match name {
                    "list_threads" => self.list_threads_tool(project_id, args).await,
                    "create_thread" => self.create_thread_tool(project_id, args).await,
                    "rename_thread" => self.rename_thread_tool(project_id, args).await,
                    "set_thread_status" => self.set_thread_status_tool(project_id, args).await,
                    "archive_thread" => self.archive_thread_tool(project_id, args).await,
                    "delete_thread" => self.delete_thread_tool(project_id, args).await,
                    "send_thread_message" => self.send_thread_message_tool(project_id, args).await,
                    "read_thread_updates" => self.read_thread_updates_tool(project_id, args).await,
                    "delivery_validate_target" => {
                        let target_branch = match Self::trimmed_string(args, "target_branch") {
                            Some(branch) => branch.to_string(),
                            None => {
                                return ToolResult::err_fmt(
                                    "Missing required parameter: target_branch",
                                );
                            }
                        };
                        let remote_url =
                            Self::trimmed_string(args, "remote_url").map(ToOwned::to_owned);
                        match delivery::validate_delivery_target(
                            project_id,
                            route_id,
                            target_branch,
                            remote_url,
                        )
                        .await
                        {
                            Ok(validation) => ToolResult::ok(json!({
                                "route": { "id": route.id, "name": route.name },
                                "validation": validation,
                            })),
                            Err(error) => ToolResult::err(json!({ "error": error })),
                        }
                    }
                    "delivery_get_versions" => {
                        match delivery::get_board_versions(project_id, route_id).await {
                            Ok(versions) => ToolResult::ok(json!({
                                "route": { "id": route.id, "name": route.name },
                                "versions": versions,
                            })),
                            Err(error) => ToolResult::err(json!({ "error": error })),
                        }
                    }
                    "delivery_get_latest_version" => {
                        match delivery::get_latest_board_version(project_id, route_id).await {
                            Ok(version) => ToolResult::ok(json!({
                                "route": { "id": route.id, "name": route.name },
                                "version": version,
                            })),
                            Err(error) => ToolResult::err(json!({ "error": error })),
                        }
                    }
                    "delivery_get_current" => {
                        match delivery::get_current_board_delivery(project_id, route_id).await {
                            Ok(current) => ToolResult::ok(json!({
                                "route": { "id": route.id, "name": route.name },
                                "delivery": current,
                            })),
                            Err(error) => ToolResult::err(json!({ "error": error })),
                        }
                    }
                    "delivery_start" => self.start_delivery(project_id, route_id, args).await,
                    "delivery_publish" => self.publish_delivery(project_id, route_id, args).await,
                    "delivery_merge" => self.merge_delivery(project_id, route_id, args).await,
                    "delivery_get_attempts" => {
                        let delivery_id = match Self::arg_i64(args, "delivery_id") {
                            Some(delivery_id) => delivery_id,
                            None => {
                                return ToolResult::err_fmt(
                                    "Missing required parameter: delivery_id",
                                );
                            }
                        };
                        match delivery::get_delivery_attempts(project_id, route_id, delivery_id)
                            .await
                        {
                            Ok(attempts) => ToolResult::ok(json!({ "attempts": attempts })),
                            Err(error) => ToolResult::err(json!({ "error": error })),
                        }
                    }
                    "delivery_retry" => {
                        let delivery_id = match Self::arg_i64(args, "delivery_id") {
                            Some(delivery_id) => delivery_id,
                            None => {
                                return ToolResult::err_fmt(
                                    "Missing required parameter: delivery_id",
                                );
                            }
                        };
                        match delivery::retry_board_delivery(project_id, route_id, delivery_id)
                            .await
                        {
                            Ok(attempt) => ToolResult::ok(json!({ "attempt": attempt })),
                            Err(error) => ToolResult::err(json!({ "error": error })),
                        }
                    }
                    "delivery_complete" => {
                        let delivery_id = match Self::arg_i64(args, "delivery_id") {
                            Some(delivery_id) => delivery_id,
                            None => {
                                return ToolResult::err_fmt(
                                    "Missing required parameter: delivery_id",
                                );
                            }
                        };
                        let action = match Self::trimmed_string(args, "action") {
                            Some(action) => action.to_string(),
                            None => {
                                return ToolResult::err_fmt("Missing required parameter: action");
                            }
                        };
                        let summary = Self::trimmed_string(args, "summary").map(ToOwned::to_owned);
                        let remote_url =
                            Self::trimmed_string(args, "remote_url").map(ToOwned::to_owned);
                        match delivery::complete_board_delivery(
                            project_id,
                            route_id,
                            delivery_id,
                            action,
                            summary,
                            remote_url,
                        )
                        .await
                        {
                            Ok(current) => ToolResult::ok(json!({ "delivery": current })),
                            Err(error) => ToolResult::err(json!({ "error": error })),
                        }
                    }
                    "delivery_abandon" => {
                        let delivery_id = match self
                            .resolve_existing_delivery_id(project_id, route_id, args)
                            .await
                        {
                            Ok(delivery_id) => delivery_id,
                            Err(error) => return error,
                        };
                        match delivery::abandon_board_delivery(project_id, route_id, delivery_id)
                            .await
                        {
                            Ok(()) => ToolResult::ok(json!({
                                "success": true,
                                "delivery_id": delivery_id,
                            })),
                            Err(error) => ToolResult::err(json!({ "error": error })),
                        }
                    }
                    _ => ToolResult::err(json!({ "error": format!("Unknown tool: {}", name) })),
                }
            }
        }
    }
}
