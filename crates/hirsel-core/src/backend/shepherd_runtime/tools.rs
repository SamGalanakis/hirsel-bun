use std::path::{Path, PathBuf};

use crate::backend::project::ProjectStore;
use crate::backend::text_patch::TEXT_PATCH_INSTRUCTIONS;
use crate::backend::tool_results::edit_result_with;
use crate::backend::ProjectWorkspaceEntry;
use crate::backend::{ShepherdChatMessage, ShepherdThread, ShepherdThreadStore};
use globset::{Glob, GlobSetBuilder};
use lash::{PromptContribution, ToolDefinition, ToolParam, ToolProvider, ToolResult};
use serde_json::Map;
use serde_json::{json, Value};
use walkdir::WalkDir;

use super::commands::{
    archive_thread, close_thread_port_forward, create_thread, delete_thread, forward_thread_port,
    list_thread_port_forwards, send_scope_message,
};
use super::queries::{get_thread_activity, get_thread_conversation};
use super::types::ShepherdScope;

const NODE_READ_DEFAULT_LIMIT: usize = 2000;
const NODE_READ_MAX_LINE_LEN: usize = 2000;

#[derive(Clone, Debug)]
pub(super) struct DesktopAppHandle;

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
            // If the LLM passes an absolute path that starts with the workspace root,
            // silently strip the prefix and treat it as relative.
            let value = if let Some(stripped) = value.strip_prefix(root.to_str().unwrap_or("")) {
                let stripped = stripped.strip_prefix('/').unwrap_or(stripped);
                if stripped.is_empty() {
                    "."
                } else {
                    stripped
                }
            } else {
                value
            };
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

    async fn list_project_workspaces(&self, project_id: i64) -> ToolResult {
        let store = match ProjectStore::open().await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };
        match store.get_project(project_id).await {
            Ok(project) => ToolResult::ok(json!({
                "project_id": project_id,
                "workspaces": project.workspaces,
                "shepherd_cwd": project.shepherd_cwd,
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn upsert_project_workspace(&self, project_id: i64, args: &Value) -> ToolResult {
        let id = match Self::trimmed_string(args, "id") {
            Some(value) => value.to_string(),
            None => return ToolResult::err_fmt("Missing required parameter: id"),
        };
        let kind = match Self::trimmed_string(args, "kind") {
            Some(value) => value.to_string(),
            None => return ToolResult::err_fmt("Missing required parameter: kind"),
        };
        let label = match Self::trimmed_string(args, "label") {
            Some(value) => value.to_string(),
            None => return ToolResult::err_fmt("Missing required parameter: label"),
        };
        let path = Self::trimmed_string(args, "path").map(ToOwned::to_owned);
        let url = Self::trimmed_string(args, "url").map(ToOwned::to_owned);
        let branch = Self::trimmed_string(args, "branch").map(ToOwned::to_owned);

        let store = match ProjectStore::open().await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };
        let workspace = ProjectWorkspaceEntry {
            id,
            kind,
            label,
            path,
            url,
            branch,
        };
        match store.upsert_workspace(project_id, workspace).await {
            Ok(project) => ToolResult::ok(json!({
                "project_id": project_id,
                "workspaces": project.workspaces,
                "shepherd_cwd": project.shepherd_cwd,
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn remove_project_workspace(&self, project_id: i64, args: &Value) -> ToolResult {
        let id = match Self::trimmed_string(args, "id") {
            Some(value) => value,
            None => return ToolResult::err_fmt("Missing required parameter: id"),
        };
        let store = match ProjectStore::open().await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };
        match store.remove_workspace(project_id, id).await {
            Ok(project) => ToolResult::ok(json!({
                "project_id": project_id,
                "workspaces": project.workspaces,
                "shepherd_cwd": project.shepherd_cwd,
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn glob_workspace(&self, args: &Value) -> ToolResult {
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
        let limit = match Self::parse_limit(args) {
            Ok(Some(limit)) => limit,
            Ok(None) => usize::MAX,
            Err(error) => return error,
        };

        let mut builder = GlobSetBuilder::new();
        let glob = match Glob::new(&pattern) {
            Ok(glob) => glob,
            Err(error) => {
                return ToolResult::err(json!({
                    "error": format!("Invalid glob pattern '{}': {}", pattern, error)
                }));
            }
        };
        builder.add(glob);
        let matcher = match builder.build() {
            Ok(set) => set,
            Err(error) => {
                return ToolResult::err(json!({
                    "error": format!("Failed to build glob matcher: {}", error)
                }));
            }
        };

        let mut items = Vec::new();
        for entry in WalkDir::new(&base)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if entry.path() == base {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(root) else {
                continue;
            };
            let rel_str = rel.display().to_string();
            if !matcher.is_match(&rel_str) {
                continue;
            }

            items.push(json!({
                "path": rel_str,
                "is_dir": entry.file_type().is_dir(),
            }));

            if items.len() >= limit {
                break;
            }
        }

        ToolResult::ok(json!({
            "pattern": pattern,
            "items": items,
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
        crate::backend::plans::extract_latest_plan(messages)
            .and_then(|plan| serde_json::to_value(plan).ok())
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
                .update_thread(&thread.id, None, None, None, Some(status), None)
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
            .update_thread(&thread.id, Some(new_title), None, None, None, None)
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
            .update_thread(&thread.id, None, None, summary, Some(status), None)
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
                )
                .await;
        }
        match send_scope_message(
            ShepherdScope::Thread {
                project_id,
                thread_id: thread.id.clone(),
                title: thread.title.clone(),
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
    async fn highlight_thread_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let thread = match self.resolve_thread(project_id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        let message = match Self::trimmed_string(args, "message") {
            Some(msg) => msg,
            None => return ToolResult::err_fmt("Missing required parameter: message"),
        };
        let store = match self.thread_store().await {
            Ok(store) => store,
            Err(error) => return error,
        };
        match store.set_highlight(&thread.id, Some(message)).await {
            Ok(()) => ToolResult::ok(json!({
                "thread_id": thread.id,
                "highlight": message,
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn dismiss_highlight_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let thread = match self.resolve_thread(project_id, args).await {
            Ok(thread) => thread,
            Err(error) => return error,
        };
        let store = match self.thread_store().await {
            Ok(store) => store,
            Err(error) => return error,
        };
        match store.set_highlight(&thread.id, None).await {
            Ok(()) => ToolResult::ok(json!({
                "thread_id": thread.id,
                "highlight": null,
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn search_threads_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let query = match Self::trimmed_string(args, "query") {
            Some(q) => q.to_string(),
            None => return ToolResult::err_fmt("Missing required parameter: query"),
        };
        let thread_id_filter = Self::trimmed_string(args, "thread_id").map(ToOwned::to_owned);
        let limit = Self::arg_i64(args, "limit").unwrap_or(20).clamp(1, 100) as usize;

        let db = crate::backend::db::global_db().await;
        let prefix = if let Some(tid) = &thread_id_filter {
            format!("__thread__:{tid}")
        } else {
            format!("thread:{project_id}:")
        };

        // Search preview_text and chunks_json for the query string
        let mut response = match db
            .query(
                "SELECT id, scope_key, role, preview_text, created_at \
                 FROM shepherd_chat_message \
                 WHERE scope_key CONTAINS $prefix \
                   AND (preview_text CONTAINS $query OR chunks_json CONTAINS $query) \
                 ORDER BY created_at DESC \
                 LIMIT $limit",
            )
            .bind(("prefix", prefix))
            .bind(("query", query.clone()))
            .bind(("limit", limit as i64))
            .await
        {
            Ok(r) => r,
            Err(error) => {
                return ToolResult::err(json!({ "error": format!("Search failed: {error}") }));
            }
        };

        let rows: Vec<Value> = response.take(0).unwrap_or_default();
        ToolResult::ok(json!({
            "query": query,
            "matches": rows,
        }))
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
        "glob" => common.glob_workspace(args).await,
        "read_file" => common.read_workspace_file(args).await,
        "grep" => common.grep_workspace(args).await,
        "list_project_workspaces" => common.list_project_workspaces(project_id).await,
        "upsert_project_workspace" => common.upsert_project_workspace(project_id, args).await,
        "remove_project_workspace" => common.remove_project_workspace(project_id, args).await,
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
        "delete_thread" => common.delete_thread_tool(project_id, args).await,
        "send_thread_message" => common.send_thread_message_tool(project_id, args).await,
        "read_thread_updates" => common.read_thread_updates_tool(project_id, args).await,
        "forward_port" => common.forward_port_tool(project_id, args).await,
        "list_port_forwards" => common.list_port_forwards_tool(project_id, args).await,
        "close_port_forward" => common.close_port_forward_tool(project_id, args).await,
        "read_project_retained_context" => common.read_project_retained_context(project_id).await,
        "update_project_retained_context" => {
            common
                .update_project_retained_context(project_id, args)
                .await
        }
        "ls" => common.list_workspace(args).await,
        "read_file" => common.read_workspace_file(args).await,
        "grep" => common.grep_workspace(args).await,
        "patch_canvas_document" => {
            crate::backend::librarian::patch_canvas_document(project_id, args).await
        }
        "highlight_thread" => common.highlight_thread_tool(project_id, args).await,
        "dismiss_highlight" => common.dismiss_highlight_tool(project_id, args).await,
        "search_threads" => common.search_threads_tool(project_id, args).await,
        _ => ToolResult::err(json!({ "error": format!("Unknown tool: {}", name) })),
    }
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
                name: "glob".to_string(),
                description: "Find workspace files or directories matching a glob pattern.".to_string(),
                params: vec![
                    ToolParam::typed("pattern", "str"),
                    ToolParam::optional("path", "str"),
                    ToolParam::optional("limit", "int"),
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
                name: "list_project_workspaces".to_string(),
                description: "List the project's attached workspaces and shepherd cwd.".to_string(),
                params: vec![],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "upsert_project_workspace".to_string(),
                description: "Insert or replace one attached workspace entry on the project by stable workspace id.".to_string(),
                params: vec![
                    ToolParam::typed("id", "str"),
                    ToolParam::typed("kind", "str"),
                    ToolParam::typed("label", "str"),
                    ToolParam::optional("path", "str"),
                    ToolParam::optional("url", "str"),
                    ToolParam::optional("branch", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "remove_project_workspace".to_string(),
                description: "Remove one attached workspace entry from the project by stable workspace id.".to_string(),
                params: vec![ToolParam::typed("id", "str")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "graph_surql".to_string(),
                description: "Run an arbitrary SurrealQL query against the project knowledge graph. `$project_id` is bound automatically. Project scoping and graph write restrictions are enforced by SurrealDB permissions.".to_string(),
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
                    "Patch the `content` field on an existing knowledge-graph node. Project scoping is enforced by SurrealDB permissions.\n\n{}",
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
                description: "Create a new execution thread with its own workspace.".to_string(),
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
                description: "Expose an HTTP or HTTPS port from a thread workspace to the user and to shepherd. Labels are required.".to_string(),
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
                name: "highlight_thread".to_string(),
                description: "Draw the user's attention to a thread that needs input or review. Sets a visible highlight on the thread card.".to_string(),
                params: vec![
                    ToolParam::optional("thread_id", "str"),
                    ToolParam::optional("title", "str"),
                    ToolParam::typed("message", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "dismiss_highlight".to_string(),
                description: "Remove a highlight from a thread.".to_string(),
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
                name: "search_threads".to_string(),
                description: "Search across all thread conversations for the project. Returns matching messages with thread context.".to_string(),
                params: vec![
                    ToolParam::typed("query", "str"),
                    ToolParam::optional("thread_id", "str"),
                    ToolParam::optional("limit", "int"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "patch_canvas_document".to_string(),
                description: format!(
                    "Patch the project canvas document in place using a validated line patch. Graph-backed references must use node=\"kind:id\".\n\n{}",
                    TEXT_PATCH_INSTRUCTIONS
                ),
                params: vec![
                    ToolParam::typed("patch", "str"),
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

        execute_shepherd_tool(&self.common, project_id, name, args).await
    }
}

// ═══════════════════════════════════════
// Plugin prompt contributions
// ═══════════════════════════════════════

pub(super) fn shepherd_prompt_contributions() -> Vec<PromptContribution> {
    vec![
        PromptContribution::guidance(
            "event_processing",
            "Event Processing",
            concat!(
                "You receive two kinds of input:\n",
                "- **User messages**: respond conversationally and take action.\n",
                "- **Event batches**: thread completions, blocks, failures, and state changes.\n\n",
                "For event batches without a user message, decide what needs attention:\n",
                "- Completed threads: update the canvas with results, summarize for the user if significant.\n",
                "- Blocked threads: use `highlight_thread` to surface the blocking question.\n",
                "- Failed threads: diagnose and either retry or alert the user.\n",
                "- If nothing requires user attention, update the canvas silently and produce no chat output.\n",
            ),
        ),
        PromptContribution::guidance(
            "thread_management",
            "Thread Management",
            concat!(
                "Use a thread when isolation, parallel progress, or a separate workspace clearly helps. ",
                "Reuse an existing thread for the same line of work; otherwise create a new one. ",
                "Keep titles, statuses, and summaries short and user-understandable.\n\n",
                "Use `highlight_thread(message)` to draw the user's attention to threads needing input or review. ",
                "Use `dismiss_highlight` when the thread no longer needs attention.\n\n",
                "Use `search_threads(query)` to find relevant conversations across all project threads.",
            ),
        ),
        PromptContribution::guidance(
            "retained_context",
            "Retained Context",
            "Use `read_project_retained_context` and `update_project_retained_context` to persist project-level notes, summaries, or decisions across sessions. Keep retained context concise and up to date.",
        ),
        PromptContribution::guidance(
            "canvas_rules",
            "Canvas",
            concat!(
                "The canvas is an ephemeral project-scoped document that can reference the knowledge graph.\n",
                "Use `patch_canvas_document(patch)` to update it with plans, diagrams, or working state for the user.\n",
                "Reference graph nodes with tags like `<hirsel-node-ref node=\"feature:auth\">` instead of copying content.\n",
                "Always use a single `node=\"kind:id\"` attribute. Do not emit separate `kind=` / `id=` attributes.\n",
                "Keep the canvas concise and remove stale sections.",
            ),
        ),
    ]
}

pub(super) fn librarian_prompt_contributions() -> Vec<PromptContribution> {
    vec![
        PromptContribution::guidance(
            "knowledge_graph_schema",
            "Knowledge Graph Schema",
            concat!(
                "## Node kinds\n\n",
                "| Kind | Purpose | ID convention | Example |\n",
                "|------|---------|---------------|---------|\n",
                "| `component` | Architectural building blocks | stable slug | `auth`, `api-gateway`, `db-layer` |\n",
                "| `entity` | Domain model objects | singular slug | `user`, `workspace`, `project` |\n",
                "| `convention` | How things should be done | topic slug | `naming`, `error-handling`, `testing` |\n",
                "| `decision` | Why things are the way they are | descriptive slug | `chose-surrealdb`, `monorepo-structure` |\n",
                "| `fact` | Project-specific truths, quirks, gotchas | descriptive slug | `ci-needs-docker`, `deploy-to-fly`, `port-8484` |\n",
                "| `goal` | Active objectives or milestones | slug | `v1-launch`, `reduce-cold-start` |\n",
                "| `document` | Canvas and other project documents | slug | `canvas` |\n\n",
                "## Node fields\n\n",
                "- `kind`, `node_id`: identity (part of the record ID)\n",
                "- `label`: short human-readable title\n",
                "- `content`: the substance — write content that is **useful to a future agent that knows nothing about the project**\n",
                "- `tags`: flat string array for cross-cutting labels (e.g. `[\"auth\", \"security\"]`)\n",
                "- `source`: who produced this (`user`, `shepherd`, `librarian`)\n",
                "- `metadata`: optional JSON for kind-specific structured data\n\n",
                "## Edge relations\n\n",
                "- `part_of`: structural containment (child → parent)\n",
                "- `depends_on`: runtime or build dependency\n",
                "- `implements`: component/entity that realizes a goal\n",
                "- `relates_to`: soft association\n\n",
            ),
        ),
        PromptContribution::guidance(
            "knowledge_graph_quality",
            "Knowledge Graph Quality",
            concat!(
                "## What to persist\n\n",
                "Good graph content answers: \"What would a new agent need to know to work on this project effectively?\"\n\n",
                "- Architecture: components, how they connect, what each does\n",
                "- Domain model: key entities, their relationships, business rules\n",
                "- Conventions: naming, file layout, error handling patterns, testing approach\n",
                "- Decisions: technology choices, tradeoffs, rationale (not just \"we use X\" but \"we use X because Y\")\n",
                "- Facts: CI/CD quirks, deployment details, environment setup, external service dependencies\n",
                "- Goals: what the team is working toward, priorities\n\n",
                "## Content quality\n\n",
                "Bad: `label: \"Auth\"` content: `\"Handles authentication\"`\n",
                "Good: `label: \"Auth system\"` content: `\"JWT-based auth with RS256. Tokens expire 24h, refresh via /api/auth/refresh. Key pair in env HIRSEL_JWT_*. Login flow in src/auth/. Rate-limited to 5 attempts/min per IP.\"`\n\n",
                "## Principles\n\n",
                "- Prefer fewer, richer nodes over many thin ones\n",
                "- Update existing nodes with new information rather than creating duplicates\n",
                "- Use `UPSERT` — always\n",
                "- Use `tags` for cross-cutting concerns rather than creating edges for every association\n",
                "- Set `source` to indicate provenance (`user` for explicit user input, `shepherd` for extracted from conversation)\n",
                "- Query narrowly before updates to check what already exists\n",
            ),
        ),
        PromptContribution::guidance(
            "surrealql_guide",
            "SurrealQL Reference",
            crate::backend::librarian::LIBRARIAN_SURREALQL_GUIDE,
        ),
        PromptContribution::guidance(
            "index_maintenance",
            "Project Index",
            concat!(
                "The `document:index` node is the project map. Keep it current after syncs that add or change nodes.\n",
                "Structure it by kind (Components, Entities, Conventions, Decisions, Facts, Goals) with inline ",
                "references like [component:auth] for each catalogued node. It should serve as a springboard — ",
                "reading the index should tell a new agent what exists and where to look deeper.\n",
                "You may introduce custom node kinds per project. Document any custom kinds in the index.",
            ),
        ),
        PromptContribution::guidance(
            "workspace_management",
            "Workspace Management",
            "Keep the workspace list current when one is attached, removed, renamed, moved, or its branch/url changes. Use `list_project_workspaces`, `upsert_project_workspace`, and `remove_project_workspace` to maintain workspace metadata.",
        ),
        PromptContribution::guidance(
            "graph_text_editing",
            "Graph Text Editing",
            "Use `edit_graph_node_text(kind, id, field=\"content\", patch)` for incremental refinement of long `content` fields instead of rewriting the whole node.",
        ),
    ]
}
