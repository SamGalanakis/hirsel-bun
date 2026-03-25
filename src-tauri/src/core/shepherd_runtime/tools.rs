use std::path::{Path, PathBuf};

use crate::core::app::{delivery, events, routes, workers, worktree};
use crate::core::db::{global_pool, utc_now};
use crate::core::delta::{DeltaState, UpdateBoardNodeRequest};
use crate::core::project::{validate_project_focus_view_html, ProjectStore};
use crate::core::route::{CreateRouteRequest, Route, RouteStore};
use crate::core::WorkerConcernStore;
use crate::core::{ensure_sync_project_task, CapabilityProfile};
use lash::tools::ApplyPatchTool;
use lash::{ToolDefinition, ToolParam, ToolProvider, ToolResult};
use serde_json::{json, Value};
use walkdir::WalkDir;

const NODE_READ_DEFAULT_LIMIT: usize = 2000;
const NODE_READ_MAX_LINE_LEN: usize = 2000;
const NODE_PATCH_VIRTUAL_FILENAME: &str = "node.md";

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

    fn string_list_arg(args: &Value, key: &str) -> Option<Vec<String>> {
        args.get(key).and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_str().map(str::trim))
                    .filter(|item| !item.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
        })
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
                }))
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
                }))
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
                        }))
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
                        }))
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

    async fn get_route_work_tree(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };

        match worktree::get_route_work_tree(project_id, route.id).await {
            Ok(snapshot) => ToolResult::ok(json!(snapshot)),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn create_work_item_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let title = match Self::trimmed_string(args, "title") {
            Some(title) => title.to_string(),
            None => return ToolResult::err_fmt("Missing required parameter: title"),
        };
        let parent_id = Self::trimmed_string(args, "parent_id").map(ToOwned::to_owned);
        let description = Self::trimmed_string(args, "description").map(ToOwned::to_owned);
        let blocked_by = Self::string_list_arg(args, "blocked_by");

        match worktree::create_work_item(
            project_id,
            route.id,
            parent_id,
            title,
            description,
            blocked_by,
        )
        .await
        {
            Ok(item) => ToolResult::ok(json!({ "item": item })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn split_work_item_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let item_id = match Self::trimmed_string(args, "item_id") {
            Some(item_id) => item_id.to_string(),
            None => return ToolResult::err_fmt("Missing required parameter: item_id"),
        };
        let items = match args.get("items").and_then(|value| value.as_array()) {
            Some(items) if !items.is_empty() => items,
            _ => return ToolResult::err_fmt("Missing required parameter: items"),
        };

        let mut split_items = Vec::with_capacity(items.len());
        for item in items {
            let title = match item
                .get("title")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(title) => title,
                None => return ToolResult::err_fmt("Each split item needs a title"),
            };
            let description = item
                .get("description")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            split_items.push(worktree::SplitWorkItemRequest {
                title: title.to_string(),
                description,
            });
        }

        match worktree::split_work_item(project_id, route.id, item_id, split_items).await {
            Ok(items) => ToolResult::ok(json!({ "items": items })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn assign_work_item_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let item_id = match Self::trimmed_string(args, "item_id") {
            Some(item_id) => item_id.to_string(),
            None => return ToolResult::err_fmt("Missing required parameter: item_id"),
        };
        let agent_kind = Self::trimmed_string(args, "agent_kind").map(ToOwned::to_owned);
        let agent_id = Self::trimmed_string(args, "agent_id").map(ToOwned::to_owned);
        let capability_profile = match Self::trimmed_string(args, "capability_profile") {
            Some("channel") => Some(CapabilityProfile::Channel),
            Some("branch") => Some(CapabilityProfile::Branch),
            Some("code_worker") => Some(CapabilityProfile::CodeWorker),
            Some("ops_worker") => Some(CapabilityProfile::OpsWorker),
            Some(other) => {
                return ToolResult::err(json!({
                    "error": format!("Invalid capability_profile: {}", other),
                }))
            }
            None => None,
        };

        match worktree::assign_work_item(
            project_id,
            route.id,
            item_id,
            agent_kind,
            agent_id,
            capability_profile,
        )
        .await
        {
            Ok(item) => ToolResult::ok(json!({ "item": item })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn delegate_to_worker_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let item_id = match Self::trimmed_string(args, "item_id") {
            Some(item_id) => item_id,
            None => return ToolResult::err_fmt("Missing required parameter: item_id"),
        };
        let capability_profile = match Self::trimmed_string(args, "capability_profile") {
            Some("code_worker") => CapabilityProfile::CodeWorker,
            Some("ops_worker") => CapabilityProfile::OpsWorker,
            Some(other) => {
                return ToolResult::err(json!({
                    "error": format!("Invalid capability_profile for worker delegation: {}", other),
                }))
            }
            None => return ToolResult::err_fmt("Missing required parameter: capability_profile"),
        };
        let worker_name = Self::trimmed_string(args, "worker_name").map(ToOwned::to_owned);

        match workers::delegate_route_worker(
            project_id,
            route.id,
            item_id,
            capability_profile,
            worker_name,
        )
        .await
        {
            Ok(worker) => ToolResult::ok(json!({ "worker": worker })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn reopen_work_item_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let item_id = match Self::trimmed_string(args, "item_id") {
            Some(item_id) => item_id.to_string(),
            None => return ToolResult::err_fmt("Missing required parameter: item_id"),
        };

        match worktree::reopen_work_item(project_id, route.id, item_id).await {
            Ok(item) => ToolResult::ok(json!({ "item": item })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn archive_work_item_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let item_id = match Self::trimmed_string(args, "item_id") {
            Some(item_id) => item_id.to_string(),
            None => return ToolResult::err_fmt("Missing required parameter: item_id"),
        };

        match worktree::archive_work_item(project_id, route.id, item_id.clone()).await {
            Ok(()) => ToolResult::ok(json!({
                "success": true,
                "item_id": item_id,
            })),
            Err(error) => ToolResult::err(json!({ "error": error })),
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
        let content: String = selected
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
        ToolResult::ok(json!({
            "node_id": node.id,
            "offset": offset,
            "shown_lines": selected.len(),
            "total_lines": total_lines,
            "truncated": end_idx < total_lines,
            "next_offset": (end_idx < total_lines).then_some(end_idx + 1),
            "content": content,
        }))
    }

    async fn apply_patch_node(&self, project_id: i64, route_id: i64, args: &Value) -> ToolResult {
        let node_id = match args.get("node_id").and_then(|v| v.as_str()).map(str::trim) {
            Some(id) if !id.is_empty() => id,
            _ => return ToolResult::err_fmt("Missing required parameter: node_id"),
        };
        let input = match Self::string_arg(args, "input") {
            Ok(value) => value,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };

        let state = DeltaState::with_route(project_id, route_id);
        let node = match state.get_node(node_id).await {
            Ok(node) => node,
            Err(_) => return ToolResult::err_fmt(format_args!("Node does not exist: {}", node_id)),
        };

        let temp_dir =
            std::env::temp_dir().join(format!("hirsel-node-patch-{}", uuid::Uuid::new_v4()));
        if let Err(error) = std::fs::create_dir_all(&temp_dir) {
            return ToolResult::err(json!({
                "error": format!("failed to create temp dir for node patch: {}", error)
            }));
        }
        let virtual_path = temp_dir.join(NODE_PATCH_VIRTUAL_FILENAME);
        if let Err(error) = std::fs::write(&virtual_path, &node.content) {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return ToolResult::err(json!({
                "error": format!("failed to stage node content for patching: {}", error)
            }));
        }
        let patch_result = ApplyPatchTool
            .execute(
                "apply_patch",
                &json!({
                    "input": input,
                    "workdir": temp_dir.to_string_lossy().to_string(),
                }),
            )
            .await;
        if !patch_result.success {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return patch_result;
        }
        let new_content = match std::fs::read_to_string(&virtual_path) {
            Ok(content) => content,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&temp_dir);
                return ToolResult::err(json!({
                    "error": format!("patched node content could not be read: {}", error)
                }));
            }
        };
        let _ = std::fs::remove_dir_all(&temp_dir);

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
                    "Applied patch to {} ({} lines)",
                    updated.id,
                    updated.content.lines().count()
                ),
                "diff": Self::compact_diff(&node.content, &new_content, &updated.id, 50),
                "patch": patch_result.result,
            })),
            Err(error) => ToolResult::err_fmt(format_args!("Failed to write node: {}", error)),
        }
    }

    async fn shepherd_get_workers(&self, project_id: i64, route_id: i64) -> ToolResult {
        match workers::get_route_workers(project_id, route_id).await {
            Ok(workers) => ToolResult::ok(json!({
                "workers": workers
            })),
            Err(error) => ToolResult::err(json!({ "error": error })),
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

        match events::get_route_worker_events(
            project_id,
            route_id,
            worker_name.clone(),
            after_id,
            limit,
        )
        .await
        {
            Ok(resp) => ToolResult::ok(json!({
                "worker_name": worker_name,
                "events": resp.events,
                "last_id": resp.last_id,
                "worker_status": resp.worker_status
            })),
            Err(error) => ToolResult::err(json!({ "error": error })),
        }
    }

    async fn shepherd_get_worker_concerns(
        &self,
        project_id: i64,
        route_id: i64,
        args: &Value,
    ) -> ToolResult {
        let include_resolved = args
            .get("include_resolved")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let limit = Self::arg_i64(args, "limit");
        let store = match WorkerConcernStore::open().await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        match store
            .list_route(project_id, route_id, include_resolved, limit)
            .await
        {
            Ok(concerns) => ToolResult::ok(json!({
                "project_id": project_id,
                "route_id": route_id,
                "concerns": concerns,
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn sync_project_tool(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let refresh = args
            .get("refresh")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);

        let project_store = match ProjectStore::open().await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };
        let project = match project_store.get_project(project_id).await {
            Ok(project) => project,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        match ensure_sync_project_task(project_id, &route, &project.name, true, refresh).await {
            Ok(result) => ToolResult::ok(json!(result)),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn shepherd_resolve_worker_concern(&self, args: &Value) -> ToolResult {
        let concern_id = match Self::arg_i64(args, "concern_id") {
            Some(id) => id,
            None => return ToolResult::err_fmt("Missing required parameter: concern_id"),
        };
        let resolution = Self::trimmed_string(args, "resolution");
        let store = match WorkerConcernStore::open().await {
            Ok(store) => store,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        match store.resolve(concern_id, "shepherd", resolution).await {
            Ok(concern) => ToolResult::ok(json!({ "concern": concern })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
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
                name: "sync_project".to_string(),
                description: "Ensure the route has a real Sync project umbrella task and request that the main orchestrator perform the onboarding or refresh pass for the existing codebase.".to_string(),
                params: vec![
                    ToolParam::optional("refresh", "bool"),
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
                name: "get_route_work_tree".to_string(),
                description: "Return the current route work tree. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "create_work_item".to_string(),
                description: "Create a new work item on a route. Uses the selected route when route_name is omitted.".to_string(),
                params: vec![
                    ToolParam::typed("title", "str"),
                    ToolParam::optional("description", "str"),
                    ToolParam::optional("parent_id", "str"),
                    ToolParam::optional("blocked_by", "list"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "split_work_item".to_string(),
                description: "Split a work item into child work items. Each entry in items should include at least a title.".to_string(),
                params: vec![
                    ToolParam::typed("item_id", "str"),
                    ToolParam::typed("items", "list"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "assign_work_item".to_string(),
                description: "Assign a work item to an orchestrator or worker.".to_string(),
                params: vec![
                    ToolParam::typed("item_id", "str"),
                    ToolParam::optional("agent_kind", "str"),
                    ToolParam::optional("agent_id", "str"),
                    ToolParam::optional("capability_profile", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "reopen_work_item".to_string(),
                description: "Reopen a work item so it returns to pending.".to_string(),
                params: vec![
                    ToolParam::typed("item_id", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "archive_work_item".to_string(),
                description: "Archive a work item on a route.".to_string(),
                params: vec![
                    ToolParam::typed("item_id", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
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
                name: "shepherd_get_workers".to_string(),
                description: "List workers currently attached to a route. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "shepherd_get_worker_events".to_string(),
                description: "Get worker output or tool events for a route worker. Uses the selected route when route_name is omitted.".to_string(),
                params: vec![
                    ToolParam::typed("worker_name", "str"),
                    ToolParam::optional("after_id", "int"),
                    ToolParam::optional("limit", "int"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delegate_to_worker".to_string(),
                description: "Delegate a work item to a sandbox worker on a route. capability_profile should normally be code_worker or ops_worker.".to_string(),
                params: vec![
                    ToolParam::typed("item_id", "str"),
                    ToolParam::typed("capability_profile", "str"),
                    ToolParam::optional("worker_name", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "shepherd_get_worker_concerns".to_string(),
                description: "List worker-raised concerns and progress reports for a route. Uses the selected route when route_name is omitted.".to_string(),
                params: vec![
                    ToolParam::optional("include_resolved", "bool"),
                    ToolParam::optional("limit", "int"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "shepherd_resolve_worker_concern".to_string(),
                description: "Resolve a worker concern after the orchestrator handles it.".to_string(),
                params: vec![
                    ToolParam::typed("concern_id", "int"),
                    ToolParam::optional("resolution", "str"),
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
                name: "read_work_item".to_string(),
                description: "Read the description/content of a work item by ID.".to_string(),
                params: vec![
                    ToolParam::typed("node_id", "str"),
                    ToolParam::optional("offset", "int"),
                    ToolParam::optional("limit", "int"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "apply_patch_work_item".to_string(),
                description: "Apply an apply_patch patch to the description/content of a work item by ID.".to_string(),
                params: vec![
                    ToolParam::typed("node_id", "str"),
                    ToolParam::typed("input", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
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
            "sync_project" => self.sync_project_tool(project_id, args).await,
            "list_workspace" => self.list_workspace(args).await,
            "read_workspace_file" => self.read_workspace_file(args).await,
            "grep_workspace" => self.grep_workspace(args).await,
            "read_project_focus_view" => self.read_project_focus_view(project_id).await,
            "update_project_focus_view" => self.update_project_focus_view(project_id, args).await,
            "read_project_retained_context" => self.read_project_retained_context(project_id).await,
            "update_project_retained_context" => {
                self.update_project_retained_context(project_id, args).await
            }
            _ => {
                let route = match self.resolve_route(project_id, args).await {
                    Ok(route) => route,
                    Err(error) => return error,
                };
                let route_id = route.id;

                match name {
                    "get_route_work_tree" => self.get_route_work_tree(project_id, args).await,
                    "create_work_item" => self.create_work_item_tool(project_id, args).await,
                    "split_work_item" => self.split_work_item_tool(project_id, args).await,
                    "assign_work_item" => self.assign_work_item_tool(project_id, args).await,
                    "delegate_to_worker" => self.delegate_to_worker_tool(project_id, args).await,
                    "reopen_work_item" => self.reopen_work_item_tool(project_id, args).await,
                    "archive_work_item" => self.archive_work_item_tool(project_id, args).await,
                    "read_work_item" => self.read_node(project_id, route_id, args).await,
                    "apply_patch_work_item" => {
                        self.apply_patch_node(project_id, route_id, args).await
                    }
                    "delivery_validate_target" => {
                        let target_branch = match Self::trimmed_string(args, "target_branch") {
                            Some(branch) => branch.to_string(),
                            None => {
                                return ToolResult::err_fmt(
                                    "Missing required parameter: target_branch",
                                )
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
                                )
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
                                )
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
                                )
                            }
                        };
                        let action = match Self::trimmed_string(args, "action") {
                            Some(action) => action.to_string(),
                            None => {
                                return ToolResult::err_fmt("Missing required parameter: action")
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
                    "shepherd_get_workers" => self.shepherd_get_workers(project_id, route_id).await,
                    "shepherd_get_worker_events" => {
                        self.shepherd_get_worker_events(project_id, route_id, args)
                            .await
                    }
                    "shepherd_get_worker_concerns" => {
                        self.shepherd_get_worker_concerns(project_id, route_id, args)
                            .await
                    }
                    "shepherd_resolve_worker_concern" => {
                        self.shepherd_resolve_worker_concern(args).await
                    }
                    _ => ToolResult::err(json!({ "error": format!("Unknown tool: {}", name) })),
                }
            }
        }
    }
}
