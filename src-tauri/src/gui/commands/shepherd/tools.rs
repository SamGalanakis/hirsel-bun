use lash::tools::ApplyPatchTool;
use lash::{ToolDefinition, ToolParam, ToolProvider, ToolResult};
use serde_json::{json, Value};
use tauri::Emitter;

use crate::core::db::{global_pool, utc_now};
use crate::core::delta::{
    BoardNodeTree, CreateBoardNodeRequest, DeltaState, NodeKind, UpdateBoardNodeRequest,
};
use crate::core::orchestrator::create_orchestrator;
use crate::core::project::{validate_project_focus_view_html, ProjectStore};
use crate::core::route::{CreateRouteRequest, Route, RouteStore};
use crate::gui::commands::{delivery, delta, routes};

const NODE_READ_DEFAULT_LIMIT: usize = 2000;
const NODE_READ_MAX_LINE_LEN: usize = 2000;
const NODE_PATCH_VIRTUAL_FILENAME: &str = "node.md";

pub(super) struct ShepherdToolProvider {
    app: tauri::AppHandle,
    default_project_id: Option<i64>,
}

impl ShepherdToolProvider {
    pub(super) fn new(app: tauri::AppHandle, default_project_id: Option<i64>) -> Self {
        Self {
            app,
            default_project_id,
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

    fn parent_change_arg(args: &Value) -> Option<Option<String>> {
        args.get("parent_id").map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|parent| !parent.is_empty())
                .map(ToOwned::to_owned)
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

    async fn delete_route(&self, project_id: i64, args: &Value) -> ToolResult {
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

        match routes::delete_route(project_id, route.id).await {
            Ok(()) => {
                let selected_route = routes::get_active_route(project_id).await.ok();
                ToolResult::ok(json!({
                    "success": true,
                    "deleted_route": {
                        "id": route.id,
                        "name": route.name,
                    },
                    "selected_route": selected_route.map(|selected| json!({
                        "id": selected.id,
                        "name": selected.name,
                    })),
                    "message": format!("Deleted route '{}'.", route.name),
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

    async fn get_validatable_node_ids(&self, project_id: i64, route_id: i64) -> Vec<String> {
        DeltaState::with_route(project_id, route_id)
            .get_nodes()
            .await
            .map(|nodes| {
                nodes
                    .into_iter()
                    .filter(|node| {
                        matches!(
                            node.kind,
                            NodeKind::Task | NodeKind::Feature | NodeKind::Plan
                        )
                    })
                    .map(|node| node.id)
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn get_valid_node_ids(&self, project_id: i64, route_id: i64) -> Vec<String> {
        DeltaState::with_route(project_id, route_id)
            .get_nodes()
            .await
            .map(|nodes| nodes.into_iter().map(|node| node.id).collect())
            .unwrap_or_default()
    }

    async fn update_named_node(
        &self,
        project_id: i64,
        route_id: i64,
        id: &str,
        name: Option<&str>,
        blocked_by: Option<Vec<String>>,
        validated_by: Option<Vec<String>>,
        parent_change: Option<Option<String>>,
    ) -> ToolResult {
        let state = DeltaState::with_route(project_id, route_id);
        let old_node = match state.get_node(id).await {
            Ok(node) => node,
            Err(_) => {
                return ToolResult::ok(json!({
                    "success": false,
                    "message": format!("Node '{}' not found", id),
                    "reason": "not_found",
                    "hint": "Use board_view to see available node IDs",
                }));
            }
        };

        let was_renamed = name
            .map(|new_name| new_name != old_node.name)
            .unwrap_or(false);
        let update = UpdateBoardNodeRequest {
            name: name.map(ToOwned::to_owned),
            content: None,
            difficulty: None,
            validated_by,
            blocked_by,
        };

        if let Err(error) = state.update_node(id, &update).await {
            return ToolResult::err(json!({ "error": error.to_string() }));
        }

        if let Some(parent_id) = parent_change {
            if let Err(error) = state.move_node(id, parent_id.as_deref(), 0).await {
                return ToolResult::err(json!({ "error": error.to_string() }));
            }
        }

        if was_renamed {
            ToolResult::ok(json!({
                "success": true,
                "action": "renamed",
                "id": id,
                "message": format!("Renamed to '{}'.", name.unwrap_or_default()),
            }))
        } else {
            ToolResult::ok(json!({
                "success": true,
                "action": "updated",
                "id": id,
                "message": "Updated node.",
            }))
        }
    }

    async fn board_feature(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let id = args.get("id").and_then(|v| v.as_str());
        let name = args.get("name").and_then(|v| v.as_str());
        let blocked_by = Self::string_list_arg(args, "blocked_by");
        let validated_by = Self::string_list_arg(args, "validated_by");
        let parent_id = Self::parent_change_arg(args);
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");

        if let Some(ref deps) = blocked_by {
            let valid_ids = self.get_valid_node_ids(project_id, route.id).await;
            for dep_id in deps {
                if !valid_ids.contains(dep_id) {
                    return ToolResult::ok(json!({
                        "success": false,
                        "message": format!("Node '{}' not found in blocked_by list", dep_id),
                        "reason": "invalid_reference",
                        "valid_node_ids": valid_ids,
                    }));
                }
            }
        }

        if let Some(ref checks) = validated_by {
            let valid_ids = self.get_valid_node_ids(project_id, route.id).await;
            for check_id in checks {
                if !valid_ids.contains(check_id) {
                    return ToolResult::ok(json!({
                        "success": false,
                        "message": format!("Check '{}' not found in validated_by list", check_id),
                        "reason": "invalid_reference",
                        "valid_node_ids": valid_ids,
                    }));
                }
            }
        }

        if let Some(existing_id) = id {
            return self
                .update_named_node(
                    project_id,
                    route.id,
                    existing_id,
                    name,
                    blocked_by,
                    validated_by,
                    parent_id,
                )
                .await;
        }

        let feature_name = match name {
            Some(name) => name,
            None => return ToolResult::err_fmt("name is required for new feature"),
        };
        let req = CreateBoardNodeRequest {
            parent_id: parent_id.flatten(),
            name: feature_name.to_string(),
            kind: NodeKind::Feature,
            content: content.to_string(),
            difficulty: crate::core::delta::BoardNodeDifficulty::Medium,
            validated_by: validated_by.unwrap_or_default(),
            blocked_by: blocked_by.unwrap_or_default(),
        };

        match DeltaState::with_route(project_id, route.id)
            .create_node(&req)
            .await
        {
            Ok(node) => ToolResult::ok(json!({
                "success": true,
                "action": "created",
                "id": node.id,
                "message": format!("Created feature '{}'.", feature_name),
                "route": { "id": route.id, "name": route.name },
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn board_task(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let id = args.get("id").and_then(|v| v.as_str());
        let name = args.get("name").and_then(|v| v.as_str());
        let blocked_by = Self::string_list_arg(args, "blocked_by");
        let validated_by = Self::string_list_arg(args, "validated_by");
        let parent_id = Self::parent_change_arg(args);
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");

        if let Some(ref deps) = blocked_by {
            let valid_ids = self.get_valid_node_ids(project_id, route.id).await;
            for dep_id in deps {
                if !valid_ids.contains(dep_id) {
                    return ToolResult::ok(json!({
                        "success": false,
                        "message": format!("Task '{}' not found in blocked_by list", dep_id),
                        "reason": "invalid_reference",
                        "valid_task_ids": self.get_validatable_node_ids(project_id, route.id).await,
                    }));
                }
            }
        }

        if let Some(ref checks) = validated_by {
            let valid_ids = self.get_valid_node_ids(project_id, route.id).await;
            for check_id in checks {
                if !valid_ids.contains(check_id) {
                    return ToolResult::ok(json!({
                        "success": false,
                        "message": format!("Check '{}' not found in validated_by list", check_id),
                        "reason": "invalid_reference",
                        "valid_node_ids": valid_ids,
                    }));
                }
            }
        }

        if let Some(existing_id) = id {
            return self
                .update_named_node(
                    project_id,
                    route.id,
                    existing_id,
                    name,
                    blocked_by,
                    validated_by,
                    parent_id,
                )
                .await;
        }

        let task_name = match name {
            Some(name) => name,
            None => return ToolResult::err_fmt("name is required for new task"),
        };
        let req = CreateBoardNodeRequest {
            parent_id: parent_id.flatten(),
            name: task_name.to_string(),
            kind: NodeKind::Task,
            content: content.to_string(),
            difficulty: crate::core::delta::BoardNodeDifficulty::Medium,
            validated_by: validated_by.unwrap_or_default(),
            blocked_by: blocked_by.unwrap_or_default(),
        };

        match DeltaState::with_route(project_id, route.id)
            .create_node(&req)
            .await
        {
            Ok(node) => ToolResult::ok(json!({
                "success": true,
                "action": "created",
                "id": node.id,
                "message": format!("Created task '{}'.", task_name),
                "route": { "id": route.id, "name": route.name },
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn find_root_feature(
        &self,
        project_id: i64,
        route_id: i64,
        node_id: &str,
    ) -> Option<String> {
        let state = DeltaState::with_route(project_id, route_id);
        let mut cursor = node_id.to_string();

        loop {
            let node = state.get_node(&cursor).await.ok()?;
            match node.parent_id {
                Some(parent_id) => {
                    cursor = parent_id;
                }
                None if node.kind == NodeKind::Feature => return Some(node.id),
                None => return None,
            }
        }
    }

    async fn board_check(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let id = args.get("id").and_then(|v| v.as_str());
        let name = args.get("name").and_then(|v| v.as_str());
        let parent_id = Self::parent_change_arg(args);
        let validates = Self::string_list_arg(args, "validates");
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let route_id = route.id;

        if let Some(ref target_ids) = validates {
            let valid_ids = self.get_validatable_node_ids(project_id, route_id).await;
            for target_id in target_ids {
                if !valid_ids.contains(target_id) {
                    return ToolResult::ok(json!({
                        "success": false,
                        "message": format!("Cannot validate non-existent node '{}'", target_id),
                        "reason": "invalid_validates_reference",
                        "valid_ids": valid_ids,
                    }));
                }
            }
        }

        let state = DeltaState::with_route(project_id, route_id);
        if let Some(existing_id) = id {
            if state.get_node(existing_id).await.is_err() {
                return ToolResult::ok(json!({
                    "success": false,
                    "message": format!("Check '{}' not found", existing_id),
                    "reason": "not_found",
                    "hint": "Use board_view to see available check IDs",
                }));
            }

            let update = UpdateBoardNodeRequest {
                name: name.map(ToOwned::to_owned),
                ..Default::default()
            };
            if let Err(error) = state.update_node(existing_id, &update).await {
                return ToolResult::err(json!({ "error": error.to_string() }));
            }

            if let Some(target_ids) = validates {
                let all_nodes = match state.get_nodes().await {
                    Ok(nodes) => nodes,
                    Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
                };

                for node in &all_nodes {
                    if matches!(node.kind, NodeKind::Task | NodeKind::Feature)
                        && node.validated_by.contains(&existing_id.to_string())
                        && !target_ids.contains(&node.id)
                    {
                        let new_validated_by = node
                            .validated_by
                            .iter()
                            .filter(|candidate| candidate.as_str() != existing_id)
                            .cloned()
                            .collect::<Vec<_>>();
                        let update = UpdateBoardNodeRequest {
                            validated_by: Some(new_validated_by),
                            ..Default::default()
                        };
                        let _ = state.update_node(&node.id, &update).await;
                    }
                }

                for target_id in &target_ids {
                    let target = match state.get_node(target_id).await {
                        Ok(target) => target,
                        Err(_) => continue,
                    };
                    let mut new_validated_by = target.validated_by.clone();
                    if !new_validated_by.contains(&existing_id.to_string()) {
                        new_validated_by.push(existing_id.to_string());
                    }
                    let update = UpdateBoardNodeRequest {
                        validated_by: Some(new_validated_by),
                        ..Default::default()
                    };
                    let _ = state.update_node(target_id, &update).await;
                }
            }

            return ToolResult::ok(json!({
                "success": true,
                "action": "updated",
                "id": existing_id,
                "message": "Updated check.",
                "route": { "id": route_id, "name": route.name },
            }));
        }

        let check_name = match name {
            Some(name) => name,
            None => return ToolResult::err_fmt("name is required for new check"),
        };

        let parent_id = match parent_id {
            Some(parent_id) => parent_id,
            None => match validates.as_ref().and_then(|targets| targets.first()) {
                Some(first_target) => {
                    self.find_root_feature(project_id, route_id, first_target)
                        .await
                }
                None => None,
            },
        };

        let req = CreateBoardNodeRequest {
            parent_id,
            name: check_name.to_string(),
            kind: NodeKind::Check,
            content: content.to_string(),
            difficulty: crate::core::delta::BoardNodeDifficulty::Medium,
            validated_by: vec![],
            blocked_by: vec![],
        };

        let node = match state.create_node(&req).await {
            Ok(node) => node,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        for target_id in validates.unwrap_or_default() {
            let target = match state.get_node(&target_id).await {
                Ok(target) => target,
                Err(_) => continue,
            };
            let mut new_validated_by = target.validated_by.clone();
            if !new_validated_by.contains(&node.id) {
                new_validated_by.push(node.id.clone());
            }
            let update = UpdateBoardNodeRequest {
                validated_by: Some(new_validated_by),
                ..Default::default()
            };
            let _ = state.update_node(&target_id, &update).await;
        }

        ToolResult::ok(json!({
            "success": true,
            "action": "created",
            "id": node.id,
            "message": format!("Created check '{}'.", check_name),
            "route": { "id": route_id, "name": route.name },
        }))
    }

    async fn count_references_to(&self, project_id: i64, route_id: i64, id: &str) -> usize {
        DeltaState::with_route(project_id, route_id)
            .get_nodes()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|node| {
                node.blocked_by.iter().any(|value| value == id)
                    || node.validates.iter().any(|value| value == id)
            })
            .count()
    }

    async fn board_delete(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let id = match Self::trimmed_string(args, "id") {
            Some(id) => id,
            None => return ToolResult::err_fmt("Missing required parameter: id"),
        };
        let state = DeltaState::with_route(project_id, route.id);

        let node = match state.get_node(id).await {
            Ok(node) => node,
            Err(_) => {
                return ToolResult::ok(json!({
                    "success": false,
                    "message": format!("Node '{}' not found", id),
                    "reason": "not_found",
                    "hint": "Use board_view to see available node IDs",
                }));
            }
        };

        let refs_cleaned = self.count_references_to(project_id, route.id, id).await;
        match state.delete_node(id).await {
            Ok(_) => ToolResult::ok(json!({
                "success": true,
                "action": "deleted",
                "id": id,
                "refs_cleaned": refs_cleaned,
                "message": format!("Deleted '{}'. Removed from {} references.", node.name, refs_cleaned),
                "route": { "id": route.id, "name": route.name },
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    async fn board_requeue_node(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let node_id = match Self::trimmed_string(args, "node_id") {
            Some(node_id) => node_id,
            None => return ToolResult::err_fmt("Missing required parameter: node_id"),
        };
        let state = DeltaState::with_route(project_id, route.id);

        let node = match state.get_node(node_id).await {
            Ok(node) => node,
            Err(_) => {
                return ToolResult::ok(json!({
                    "success": false,
                    "message": format!("Node '{}' not found", node_id),
                    "reason": "not_found",
                    "hint": "Use board_view to see available node IDs",
                }));
            }
        };

        match state.reopen_node(node_id).await {
            Ok(_) => ToolResult::ok(json!({
                "success": true,
                "action": "requeued",
                "id": node_id,
                "message": format!("Requeued '{}' back to pending.", node.name),
                "route": { "id": route.id, "name": route.name },
            })),
            Err(error) => ToolResult::err(json!({ "error": error.to_string() })),
        }
    }

    fn tree_to_view_node(node: &BoardNodeTree) -> Value {
        let children = node
            .children
            .iter()
            .filter(|child| child.kind != NodeKind::Check)
            .map(Self::tree_to_view_node)
            .collect::<Vec<_>>();

        json!({
            "id": node.id,
            "name": node.name,
            "kind": node.kind.as_str(),
            "status": node.status.as_str(),
            "blocked_by": node.blocked_by,
            "children": children,
        })
    }

    fn count_nodes(tree: &[BoardNodeTree]) -> usize {
        tree.iter()
            .filter(|node| node.kind != NodeKind::Check)
            .map(|node| 1 + Self::count_nodes(&node.children))
            .sum()
    }

    async fn board_view(&self, project_id: i64, args: &Value) -> ToolResult {
        let route = match self.resolve_route(project_id, args).await {
            Ok(route) => route,
            Err(error) => return error,
        };
        let state = DeltaState::with_route(project_id, route.id);
        let board_tree = match state.get_tree().await {
            Ok(tree) => tree,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };
        let all_nodes = match state.get_nodes().await {
            Ok(nodes) => nodes,
            Err(error) => return ToolResult::err(json!({ "error": error.to_string() })),
        };

        let tasks = board_tree
            .iter()
            .filter(|node| node.kind != NodeKind::Check)
            .map(Self::tree_to_view_node)
            .collect::<Vec<_>>();
        let checks = all_nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Check)
            .map(|node| {
                json!({
                    "id": node.id,
                    "name": node.name,
                    "validates": node.validates,
                })
            })
            .collect::<Vec<_>>();

        ToolResult::ok(json!({
            "route": { "id": route.id, "name": route.name },
            "tasks": tasks,
            "checks": checks,
            "summary": format!("{} nodes, {} checks", Self::count_nodes(&board_tree), checks.len()),
        }))
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

        let orch = match create_orchestrator() {
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

        let orch = match create_orchestrator() {
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
                let _ = self.app.emit(
                    "project-focus-view-updated",
                    json!({
                        "projectId": project_id,
                        "updatedAt": view.updated_at,
                    }),
                );
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
                let _ = self.app.emit(
                    "project-retained-context-updated",
                    json!({
                        "projectId": project_id,
                        "updatedAt": context.updated_at,
                    }),
                );
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
        vec![
            ToolDefinition {
                name: "board_routes".to_string(),
                description: "List routes for the project and show which route is currently selected.".to_string(),
                params: vec![ToolParam::optional("project_id", "int")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "board_create_route".to_string(),
                description: "Create a new route by name. Forks from parent_route_name when provided; otherwise forks from the selected route.".to_string(),
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
            ToolDefinition {
                name: "board_set_active_route".to_string(),
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
            ToolDefinition {
                name: "board_delete_route".to_string(),
                description: "Delete a route by unique name. If the deleted route was selected, the project falls back to another route automatically.".to_string(),
                params: vec![
                    ToolParam::typed("name", "str"),
                    ToolParam::optional("project_id", "int"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "board_view".to_string(),
                description: "View the full board structure for a route. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "board_feature".to_string(),
                description: "Create or update a feature on a route. Uses route_name when provided, otherwise the selected route.".to_string(),
                params: vec![
                    ToolParam::optional("id", "str"),
                    ToolParam::optional("name", "str"),
                    ToolParam::optional("blocked_by", "list"),
                    ToolParam::optional("validated_by", "list"),
                    ToolParam::optional("parent_id", "str"),
                    ToolParam::optional("content", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "board_task".to_string(),
                description: "Create or update an implementation task on a route. Uses route_name when provided, otherwise the selected route.".to_string(),
                params: vec![
                    ToolParam::optional("id", "str"),
                    ToolParam::optional("name", "str"),
                    ToolParam::optional("blocked_by", "list"),
                    ToolParam::optional("validated_by", "list"),
                    ToolParam::optional("parent_id", "str"),
                    ToolParam::optional("content", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "board_check".to_string(),
                description: "Create or update a check on a route. Uses route_name when provided, otherwise the selected route.".to_string(),
                params: vec![
                    ToolParam::optional("id", "str"),
                    ToolParam::optional("name", "str"),
                    ToolParam::optional("parent_id", "str"),
                    ToolParam::optional("validates", "list"),
                    ToolParam::optional("content", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "board_delete".to_string(),
                description: "Delete a node from a route by ID. Uses route_name when provided, otherwise the selected route.".to_string(),
                params: vec![
                    ToolParam::typed("id", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "board_requeue_node".to_string(),
                description: "Requeue a node on a route back to pending. Uses route_name when provided, otherwise the selected route.".to_string(),
                params: vec![
                    ToolParam::typed("node_id", "str"),
                    ToolParam::optional("project_id", "int"),
                    ToolParam::optional("route_name", "str"),
                ],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "shepherd_start_run".to_string(),
                description: "Dispatch board nodes and start or continue execution for a route. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "shepherd_get_project_run".to_string(),
                description: "Get current run metadata for a route. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
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
            ToolDefinition {
                name: "delivery_get_versions".to_string(),
                description: "List published board versions available for delivery on a route. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "delivery_get_latest_version".to_string(),
                description: "Get the latest board version for a route. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "delivery_get_current".to_string(),
                description: "Get the current non-terminal delivery for a route. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
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
            ToolDefinition {
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
            ToolDefinition {
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
            ToolDefinition {
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
            ToolDefinition {
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
            ToolDefinition {
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
            ToolDefinition {
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
            ToolDefinition {
                name: "shepherd_get_workers".to_string(),
                description: "List workers for a route's current run. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "shepherd_get_worker_events".to_string(),
                description: "Get worker output or tool events for a route's current run. Uses the selected route when route_name is omitted.".to_string(),
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
            ToolDefinition {
                name: "read_node".to_string(),
                description: "Read node content using node_id instead of a file path. Uses the selected route when route_name is omitted.".to_string(),
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
            ToolDefinition {
                name: "apply_patch_node".to_string(),
                description: "Apply an apply_patch-format patch to node content. Uses the selected route when route_name is omitted.".to_string(),
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
            ToolDefinition {
                name: "read_project_focus_view".to_string(),
                description: "Read the current project-focus HTML artifact for this project.".to_string(),
                params: vec![ToolParam::optional("project_id", "int")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "update_project_focus_view".to_string(),
                description: "Replace the project-focus HTML artifact for this project with a full self-contained HTML document.".to_string(),
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
            ToolDefinition {
                name: "read_project_retained_context".to_string(),
                description: "Read the current project-level retained context markdown for this project.".to_string(),
                params: vec![ToolParam::optional("project_id", "int")],
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
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
            ToolDefinition {
                name: "shepherd_get_board_tree".to_string(),
                description: "Return the full board tree payload for a route. Uses the selected route when route_name is omitted.".to_string(),
                params: Self::route_param_defs(),
                returns: "dict".to_string(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
        ]
    }

    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        let project_id = match self.resolve_project_id(args) {
            Ok(project_id) => project_id,
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };

        match name {
            "board_routes" => self.list_routes(project_id).await,
            "board_create_route" => self.create_route(project_id, args).await,
            "board_set_active_route" => self.select_route(project_id, args).await,
            "board_delete_route" => self.delete_route(project_id, args).await,
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
                    "board_view" => self.board_view(project_id, args).await,
                    "board_feature" => self.board_feature(project_id, args).await,
                    "board_task" => self.board_task(project_id, args).await,
                    "board_check" => self.board_check(project_id, args).await,
                    "board_delete" => self.board_delete(project_id, args).await,
                    "board_requeue_node" => self.board_requeue_node(project_id, args).await,
                    "shepherd_start_run" => {
                        match delta::start_shepherd_run(project_id, route_id).await {
                            Ok(resp) => ToolResult::ok(json!(resp)),
                            Err(error) => ToolResult::err(json!({ "error": error })),
                        }
                    }
                    "shepherd_get_project_run" => {
                        match delta::get_project_run(project_id, route_id).await {
                            Ok(resp) => ToolResult::ok(json!(resp)),
                            Err(error) => ToolResult::err(json!({ "error": error })),
                        }
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
                    "read_node" => self.read_node(project_id, route_id, args).await,
                    "apply_patch_node" => self.apply_patch_node(project_id, route_id, args).await,
                    "shepherd_get_board_tree" => {
                        match delta::get_board_tree(project_id, route_id).await {
                            Ok(resp) => ToolResult::ok(json!(resp)),
                            Err(error) => ToolResult::err(json!({ "error": error })),
                        }
                    }
                    _ => ToolResult::err(json!({ "error": format!("Unknown tool: {}", name) })),
                }
            }
        }
    }
}
