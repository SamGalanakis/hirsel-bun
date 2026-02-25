//! Board MCP server for Shepherd agent
//!
//! Exposes board structure manipulation via MCP tools. The agent uses these tools
//! instead of editing board.json directly, which prevents ID collisions and
//! invalid structure.
//!
//! ## Tools
//!
//! - `board_view` - View full board structure with node IDs
//! - `board_feature` - Create or update a feature (high-level goal, dispatch unit)
//! - `board_task` - Create or update a task (specific implementation work)
//! - `board_check` - Create or update a check (validation)
//! - `board_delete` - Delete a node

use serde_json::{json, Value};
use std::future::Future;

use crate::core::delta::{BoardNodeTree, DeltaState, NodeKind};
use crate::core::mcp::{run_mcp_server, McpToolServer, Tool};
use crate::core::project::ProjectStore;
use crate::core::route::CreateRouteRequest;
use crate::core::route::RouteStore;
use std::sync::Mutex;

/// Block on an async future in a sync context
fn block_on<F: Future>(f: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(f)),
        Err(_) => tokio::runtime::Runtime::new()
            .expect("Failed to create tokio runtime")
            .block_on(f),
    }
}

/// Get the list of available board MCP tools
fn get_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "board_routes",
            description: "List all routes for this project. Shows route hierarchy, which routes forked from which.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "board_switch_route",
            description:
                "Switch to a different route by ID for this Shepherd session. Use board_set_active_route(name) to persist project active route.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "route_id": {
                        "type": "integer",
                        "description": "Route ID to switch to"
                    }
                },
                "required": ["route_id"]
            }),
        },
        Tool {
            name: "board_create_route",
            description: "Create a new route by name. Forks from parent_route_name when provided; otherwise forks from current route.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Unique route name to create"
                    },
                    "parent_route_name": {
                        "type": "string",
                        "description": "Existing route name to fork from (defaults to current route)"
                    }
                },
                "required": ["name"]
            }),
        },
        Tool {
            name: "board_set_active_route",
            description: "Set project active route by unique route name and switch this session to it.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Existing route name"
                    }
                },
                "required": ["name"]
            }),
        },
        Tool {
            name: "board_view",
            description: "View the full board structure with all features, tasks, and checks. Returns JSON with node hierarchy and check list.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "board_feature",
            description: "Create or update a feature — a high-level goal. On dispatch, each feature gets a dedicated plan worker that decomposes it into implementation tasks and checks.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Existing feature ID (omit for new)"
                    },
                    "name": {
                        "type": "string",
                        "description": "Feature name (required for create)"
                    },
                    "blocked_by": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Node IDs this depends on"
                    },
                    "validated_by": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Check IDs that validate this feature. Feature is Validated only when ALL listed checks pass."
                    },
                    "parent_id": {
                        "type": "string",
                        "description": "Parent node ID (null = root)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Initial content (create only)"
                    }
                },
                "required": []
            }),
        },
        Tool {
            name: "board_task",
            description: "Create or update an implementation task. Omit 'id' to create new. Returns the new/updated task ID. Tasks skip the planning phase — they are dispatched directly to workers.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Existing task ID (omit for new)"
                    },
                    "name": {
                        "type": "string",
                        "description": "Task name (required for create)"
                    },
                    "blocked_by": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Task IDs this depends on"
                    },
                    "validated_by": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Check IDs that validate this task. Task is Validated only when ALL listed checks pass."
                    },
                    "parent_id": {
                        "type": "string",
                        "description": "Parent task ID for subtasks (null = root)"
                    },
                    "position": {
                        "type": "integer",
                        "description": "Order among siblings"
                    },
                    "content": {
                        "type": "string",
                        "description": "Initial content (create only)"
                    }
                },
                "required": []
            }),
        },
        Tool {
            name: "board_check",
            description: "Create or update a check (validation node). Omit 'id' to create new. Returns the new/updated check ID. 'validates' is convenience sugar — when provided, writes validated_by on each referenced feature/task. Parent under a feature for scoped checks; omit parent for global/e2e checks.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Existing check ID (omit for new)"
                    },
                    "name": {
                        "type": "string",
                        "description": "Check name (required for create)"
                    },
                    "parent_id": {
                        "type": "string",
                        "description": "Parent node ID. Place under a feature for scoped checks. Omit for global/e2e checks."
                    },
                    "validates": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Feature/task IDs this check validates (convenience sugar — writes validated_by on targets). Optional for global/e2e checks."
                    },
                    "content": {
                        "type": "string",
                        "description": "Initial content (create only)"
                    }
                },
                "required": []
            }),
        },
        Tool {
            name: "board_delete",
            description: "Delete a node by ID. Removes the node and cleans up references. Root nodes cannot be deleted.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "ID of task or eval to delete"
                    }
                },
                "required": ["id"]
            }),
        },
        Tool {
            name: "board_requeue_node",
            description: "Requeue a node by ID (set status back to pending and clear claim/completion metadata).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "node_id": {
                        "type": "string",
                        "description": "Node ID to requeue"
                    }
                },
                "required": ["node_id"]
            }),
        },
    ]
}

/// Board MCP Server
pub struct BoardMcpServer {
    project_id: i64,
    route_id: Mutex<i64>,
}

impl BoardMcpServer {
    /// Create a new board MCP server for a project
    ///
    /// Uses the project's active route for all operations.
    pub fn new(project_id: i64) -> Self {
        // Look up the project's active route
        let route_id = block_on(async {
            let project_store = ProjectStore::open().await.ok()?;
            let project = project_store.get_project(project_id).await.ok()?;
            let route_id = project.active_route_id?;

            Some(route_id)
        })
        .unwrap_or(1);

        Self {
            project_id,
            route_id: Mutex::new(route_id),
        }
    }

    /// Get a DeltaState for the current route
    fn state(&self) -> DeltaState {
        let route_id = *self.route_id.lock().unwrap();
        DeltaState::with_route(self.project_id, route_id)
    }

    /// Get the current route ID
    fn current_route_id(&self) -> i64 {
        *self.route_id.lock().unwrap()
    }

    /// Switch to a different route
    fn switch_route(&self, route_id: i64) {
        *self.route_id.lock().unwrap() = route_id;
    }

    /// Get all valid feature/task IDs (nodes that checks can validate)
    fn get_validatable_node_ids(&self) -> Vec<String> {
        block_on(self.state().get_nodes())
            .map(|nodes| {
                nodes
                    .iter()
                    .filter(|n| {
                        n.kind == NodeKind::Task
                            || n.kind == NodeKind::Feature
                            || n.kind == NodeKind::Plan
                    })
                    .map(|n| n.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all valid node IDs (tasks and evals)
    fn get_valid_node_ids(&self) -> Vec<String> {
        block_on(self.state().get_nodes())
            .map(|nodes| nodes.iter().map(|n| n.id.clone()).collect())
            .unwrap_or_default()
    }

    // =========================================================================
    // Tool Handlers
    // =========================================================================

    /// Handle board_routes - list all routes with their relationships
    fn handle_routes(&self) -> Result<String, String> {
        let routes = block_on(async {
            let store = RouteStore::new(self.project_id)
                .await
                .map_err(|e| e.to_string())?;
            store.list_routes().await.map_err(|e| e.to_string())
        })?;

        let current_route_id = self.current_route_id();

        let routes_json: Vec<Value> = routes
            .iter()
            .map(|r| {
                let mut obj = json!({
                    "id": r.id,
                    "name": r.name,
                    "created_at": r.created_at,
                });
                if let Some(parent_id) = r.parent_route_id {
                    obj["parent_route_id"] = json!(parent_id);
                }
                if let Some(parent_version_id) = r.parent_version_id {
                    obj["forked_from_version"] = json!(parent_version_id);
                }
                if r.id == current_route_id {
                    obj["active"] = json!(true);
                }
                obj
            })
            .collect();

        Ok(serde_json::to_string_pretty(&json!({
            "routes": routes_json,
            "current_route_id": current_route_id
        }))
        .unwrap())
    }

    /// Handle board_switch_route - switch to a different route
    fn handle_switch_route(&self, args: &Value) -> Result<String, String> {
        let route_id = args
            .get("route_id")
            .and_then(|v| v.as_i64())
            .ok_or("route_id is required")?;

        // Verify route exists and get its name
        let route = block_on(async {
            let store = RouteStore::new(self.project_id)
                .await
                .map_err(|e| e.to_string())?;
            store.get_route(route_id).await.map_err(|e| e.to_string())
        })?;

        self.switch_route(route_id);

        Ok(serde_json::to_string_pretty(&json!({
            "success": true,
            "message": format!("Switched to route '{}' (id: {})", route.name, route.id),
            "route": {
                "id": route.id,
                "name": route.name
            }
        }))
        .unwrap())
    }

    /// Handle board_create_route - create a new route from parent route name/current route
    fn handle_create_route(&self, args: &Value) -> Result<String, String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("name is required")?;
        let parent_route_name = args
            .get("parent_route_name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let created = block_on(async {
            let store = RouteStore::new(self.project_id)
                .await
                .map_err(|e| e.to_string())?;
            let parent_route_id = if let Some(parent_name) = parent_route_name {
                let parent = store
                    .get_route_by_name(parent_name)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("Parent route '{}' not found", parent_name))?;
                parent.id
            } else {
                self.current_route_id()
            };

            let req = CreateRouteRequest {
                name: name.to_string(),
                parent_route_id: Some(parent_route_id),
                parent_version_id: None,
            };

            store.create_route(&req).await.map_err(|e| e.to_string())
        })?;

        Ok(serde_json::to_string_pretty(&json!({
            "success": true,
            "message": format!("Created route '{}'", created.name),
            "route": {
                "id": created.id,
                "name": created.name,
                "parent_route_id": created.parent_route_id
            }
        }))
        .unwrap())
    }

    /// Handle board_set_active_route - set project active route by name
    fn handle_set_active_route(&self, args: &Value) -> Result<String, String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("name is required")?;

        let route = block_on(async {
            let store = RouteStore::new(self.project_id)
                .await
                .map_err(|e| e.to_string())?;
            let route = store
                .get_route_by_name(name)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Route '{}' not found", name))?;

            let pool = crate::core::db::global_pool().await;
            sqlx::query("UPDATE projects SET active_route_id = ?, updated_at = ? WHERE id = ?")
                .bind(route.id)
                .bind(crate::core::db::utc_now())
                .bind(self.project_id)
                .execute(pool)
                .await
                .map_err(|e| format!("Failed to set active route: {}", e))?;

            Ok::<_, String>(route)
        })?;

        self.switch_route(route.id);

        Ok(serde_json::to_string_pretty(&json!({
            "success": true,
            "message": format!("Active route set to '{}'", route.name),
            "route": {
                "id": route.id,
                "name": route.name
            }
        }))
        .unwrap())
    }

    /// Handle board_view - returns full board structure
    fn handle_view(&self) -> Result<String, String> {
        let board_tree = block_on(self.state().get_tree()).map_err(|e| e.to_string())?;
        let all_nodes = block_on(self.state().get_nodes()).map_err(|e| e.to_string())?;

        // Convert to view format - show features and tasks (not checks, they're listed separately)
        let tasks: Vec<Value> = board_tree
            .iter()
            .filter(|n| n.kind != NodeKind::Check)
            .map(|n| self.tree_to_view_node(n))
            .collect();

        let checks: Vec<Value> = all_nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Check)
            .map(|n| {
                json!({
                    "id": n.id,
                    "name": n.name,
                    "validates": n.validates
                })
            })
            .collect();

        let node_count = self.count_nodes(&board_tree);
        let check_count = checks.len();

        Ok(json!({
            "tasks": tasks,
            "checks": checks,
            "summary": format!("{} nodes, {} checks", node_count, check_count)
        })
        .to_string())
    }

    /// Convert tree node to view format (recursive)
    fn tree_to_view_node(&self, node: &BoardNodeTree) -> Value {
        let children: Vec<Value> = node
            .children
            .iter()
            .filter(|c| c.kind != NodeKind::Check)
            .map(|c| self.tree_to_view_node(c))
            .collect();

        json!({
            "id": node.id,
            "name": node.name,
            "kind": node.kind.as_str(),
            "status": node.status.as_str(),
            "blocked_by": node.blocked_by,
            "children": children
        })
    }

    /// Count all non-check nodes in tree (recursive)
    fn count_nodes(&self, tree: &[BoardNodeTree]) -> usize {
        tree.iter()
            .filter(|n| n.kind != NodeKind::Check)
            .map(|n| 1 + self.count_node_children(n))
            .sum()
    }

    fn count_node_children(&self, node: &BoardNodeTree) -> usize {
        node.children
            .iter()
            .filter(|c| c.kind != NodeKind::Check)
            .map(|c| 1 + self.count_node_children(c))
            .sum()
    }

    /// Handle board_feature - create or update a feature
    fn handle_feature(&mut self, args: Value) -> Result<String, String> {
        let id = args.get("id").and_then(|v| v.as_str());
        let name = args.get("name").and_then(|v| v.as_str());
        let blocked_by: Option<Vec<String>> = args.get("blocked_by").and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        });
        let validated_by: Option<Vec<String>> = args.get("validated_by").and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        });
        let parent_id = args.get("parent_id").and_then(|v| v.as_str());
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");

        // Validate blocked_by references exist
        if let Some(ref deps) = blocked_by {
            let valid_ids = self.get_valid_node_ids();
            for dep_id in deps {
                if !valid_ids.contains(dep_id) {
                    return Ok(json!({
                        "success": false,
                        "message": format!("Node '{}' not found in blocked_by list", dep_id),
                        "reason": "invalid_reference",
                        "valid_node_ids": valid_ids
                    })
                    .to_string());
                }
            }
        }

        // Validate validated_by references exist (must be check nodes)
        if let Some(ref checks) = validated_by {
            let valid_ids = self.get_valid_node_ids();
            for check_id in checks {
                if !valid_ids.contains(check_id) {
                    return Ok(json!({
                        "success": false,
                        "message": format!("Check '{}' not found in validated_by list", check_id),
                        "reason": "invalid_reference",
                        "valid_node_ids": valid_ids
                    })
                    .to_string());
                }
            }
        }

        if let Some(existing_id) = id {
            self.update_task(existing_id, name, blocked_by, validated_by, parent_id)
        } else {
            let name = name.ok_or("'name' required for new feature")?;
            self.create_feature(name, blocked_by, validated_by, parent_id, content)
        }
    }

    /// Create a new feature node
    fn create_feature(
        &mut self,
        name: &str,
        blocked_by: Option<Vec<String>>,
        validated_by: Option<Vec<String>>,
        parent_id: Option<&str>,
        content: &str,
    ) -> Result<String, String> {
        use crate::core::delta::CreateBoardNodeRequest;

        let req = CreateBoardNodeRequest {
            parent_id: parent_id.map(String::from),
            name: name.to_string(),
            kind: NodeKind::Feature,
            content: content.to_string(),
            difficulty: crate::core::delta::BoardNodeDifficulty::Medium,
            validated_by: validated_by.unwrap_or_default(),
            blocked_by: blocked_by.unwrap_or_default(),
            x: None,
            y: None,
        };

        let node = block_on(self.state().create_node(&req)).map_err(|e| e.to_string())?;

        Ok(json!({
            "success": true,
            "action": "created",
            "id": node.id,
            "message": format!("Created feature '{}'.", name)
        })
        .to_string())
    }

    /// Handle board_task - create or update a task
    fn handle_task(&mut self, args: Value) -> Result<String, String> {
        let id = args.get("id").and_then(|v| v.as_str());
        let name = args.get("name").and_then(|v| v.as_str());
        let blocked_by: Option<Vec<String>> = args.get("blocked_by").and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        });
        let validated_by: Option<Vec<String>> = args.get("validated_by").and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        });
        let parent_id = args.get("parent_id").and_then(|v| v.as_str());
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");

        // Validate blocked_by references exist
        if let Some(ref deps) = blocked_by {
            let valid_ids = self.get_valid_node_ids();
            for dep_id in deps {
                if !valid_ids.contains(dep_id) {
                    return Ok(json!({
                        "success": false,
                        "message": format!("Task '{}' not found in blocked_by list", dep_id),
                        "reason": "invalid_reference",
                        "valid_task_ids": self.get_validatable_node_ids()
                    })
                    .to_string());
                }
            }
        }

        // Validate validated_by references exist (must be check nodes)
        if let Some(ref checks) = validated_by {
            let valid_ids = self.get_valid_node_ids();
            for check_id in checks {
                if !valid_ids.contains(check_id) {
                    return Ok(json!({
                        "success": false,
                        "message": format!("Check '{}' not found in validated_by list", check_id),
                        "reason": "invalid_reference",
                        "valid_node_ids": valid_ids
                    })
                    .to_string());
                }
            }
        }

        if let Some(existing_id) = id {
            // Update existing task
            self.update_task(existing_id, name, blocked_by, validated_by, parent_id)
        } else {
            // Create new task
            let name = name.ok_or("'name' required for new task")?;
            self.create_task(name, blocked_by, validated_by, parent_id, content)
        }
    }

    /// Create a new task
    fn create_task(
        &mut self,
        name: &str,
        blocked_by: Option<Vec<String>>,
        validated_by: Option<Vec<String>>,
        parent_id: Option<&str>,
        content: &str,
    ) -> Result<String, String> {
        use crate::core::delta::CreateBoardNodeRequest;

        let req = CreateBoardNodeRequest {
            parent_id: parent_id.map(String::from),
            name: name.to_string(),
            kind: NodeKind::Task,
            content: content.to_string(),
            difficulty: crate::core::delta::BoardNodeDifficulty::Medium,
            validated_by: validated_by.unwrap_or_default(),
            blocked_by: blocked_by.unwrap_or_default(),
            x: None,
            y: None,
        };

        let node = block_on(self.state().create_node(&req)).map_err(|e| e.to_string())?;

        Ok(json!({
            "success": true,
            "action": "created",
            "id": node.id,
            "message": format!("Created task '{}'.", name)
        })
        .to_string())
    }

    /// Update an existing task
    fn update_task(
        &mut self,
        id: &str,
        name: Option<&str>,
        blocked_by: Option<Vec<String>>,
        validated_by: Option<Vec<String>>,
        parent_id: Option<&str>,
    ) -> Result<String, String> {
        use crate::core::delta::UpdateBoardNodeRequest;

        // Get existing node to check for rename
        let old_node = match block_on(self.state().get_node(id)) {
            Ok(n) => n,
            Err(_) => {
                return Ok(json!({
                    "success": false,
                    "message": format!("Task '{}' not found", id),
                    "reason": "not_found",
                    "hint": "Use board_view to see available task IDs"
                })
                .to_string());
            }
        };

        // Check for rename (name changed)
        let was_renamed = name.map(|n| n != old_node.name).unwrap_or(false);

        // Build update request
        let req = UpdateBoardNodeRequest {
            name: name.map(String::from),
            content: None, // Content edited via node tools
            difficulty: None,
            validated_by,
            blocked_by,
            x: None,
            y: None,
        };

        block_on(self.state().update_node(id, &req)).map_err(|e| e.to_string())?;

        // Handle parent_id change if specified
        if let Some(new_parent) = parent_id {
            let new_parent = if new_parent.is_empty() {
                None
            } else {
                Some(new_parent)
            };
            block_on(self.state().move_node(id, new_parent, 0)).map_err(|e| e.to_string())?;
        }

        if was_renamed {
            Ok(json!({
                "success": true,
                "action": "renamed",
                "id": id,
                "message": format!("Renamed to '{}'.", name.unwrap())
            })
            .to_string())
        } else {
            Ok(json!({
                "success": true,
                "action": "updated",
                "id": id,
                "message": "Updated task."
            })
            .to_string())
        }
    }

    /// Handle board_check - create or update a check
    ///
    /// `validates` is optional convenience sugar — when provided, writes `validated_by`
    /// on each referenced node, pointing back to this check. Checks with no `validates`
    /// are global/e2e checks scheduled via `blocked_by`.
    fn handle_check(&mut self, args: Value) -> Result<String, String> {
        let id = args.get("id").and_then(|v| v.as_str());
        let name = args.get("name").and_then(|v| v.as_str());
        let parent_id = args.get("parent_id").and_then(|v| v.as_str());
        let validates: Option<Vec<String>> = args.get("validates").and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        });
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");

        // Validate validates references exist (must be spec or task nodes)
        if let Some(ref target_ids) = validates {
            let valid_ids = self.get_validatable_node_ids();
            for target_id in target_ids {
                if !valid_ids.contains(target_id) {
                    return Ok(json!({
                        "success": false,
                        "message": format!("Cannot validate non-existent node '{}'", target_id),
                        "reason": "invalid_validates_reference",
                        "valid_ids": valid_ids
                    })
                    .to_string());
                }
            }
        }

        if let Some(existing_id) = id {
            // Update existing eval
            self.update_check(existing_id, name, validates)
        } else {
            // Create new eval
            let name = name.ok_or("'name' required for new check")?;
            self.create_check(name, parent_id, validates.unwrap_or_default(), content)
        }
    }

    /// Create a new check
    ///
    /// The check node itself has no validated_by. If `validates` is provided (convenience sugar),
    /// we update each target node's validated_by to include this check.
    fn create_check(
        &mut self,
        name: &str,
        parent_id: Option<&str>,
        validates: Vec<String>,
        content: &str,
    ) -> Result<String, String> {
        use crate::core::delta::{CreateBoardNodeRequest, UpdateBoardNodeRequest};

        // Auto-parent: if no explicit parent but validates targets exist,
        // walk up from the first target to find the root feature
        let resolved_parent = match parent_id {
            Some(p) => Some(p.to_string()),
            None if !validates.is_empty() => self.find_root_feature(&validates[0]),
            None => None,
        };

        let req = CreateBoardNodeRequest {
            parent_id: resolved_parent,
            name: name.to_string(),
            kind: NodeKind::Check,
            content: content.to_string(),
            difficulty: crate::core::delta::BoardNodeDifficulty::Medium,
            validated_by: vec![], // Checks don't have validated_by
            blocked_by: vec![],
            x: None,
            y: None,
        };

        let node = block_on(self.state().create_node(&req)).map_err(|e| e.to_string())?;

        // Write validated_by on each target task (convenience sugar)
        for task_id in &validates {
            let task = match block_on(self.state().get_node(task_id)) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let mut new_validated_by = task.validated_by.clone();
            if !new_validated_by.contains(&node.id) {
                new_validated_by.push(node.id.clone());
            }
            let update = UpdateBoardNodeRequest {
                validated_by: Some(new_validated_by),
                ..Default::default()
            };
            let _ = block_on(self.state().update_node(task_id, &update));
        }

        let msg = if validates.is_empty() {
            format!("Created global check '{}'", name)
        } else {
            format!(
                "Created check '{}' validating: {}",
                name,
                validates.join(", ")
            )
        };
        Ok(json!({
            "success": true,
            "action": "created",
            "id": node.id,
            "message": msg
        })
        .to_string())
    }

    /// Update an existing check
    ///
    /// If `validates` is provided (convenience sugar), we update each target node's
    /// validated_by to include this check. This replaces the previous set of validated nodes.
    fn update_check(
        &mut self,
        id: &str,
        name: Option<&str>,
        validates: Option<Vec<String>>,
    ) -> Result<String, String> {
        use crate::core::delta::UpdateBoardNodeRequest;

        // Verify exists
        if block_on(self.state().get_node(id)).is_err() {
            return Ok(json!({
                "success": false,
                "message": format!("Check '{}' not found", id),
                "reason": "not_found",
                "hint": "Use board_view to see available check IDs"
            })
            .to_string());
        }

        // Update check node itself (name only — checks don't have validated_by)
        let req = UpdateBoardNodeRequest {
            name: name.map(String::from),
            ..Default::default()
        };

        block_on(self.state().update_node(id, &req)).map_err(|e| e.to_string())?;

        // If validates provided, update validated_by on target nodes (convenience sugar)
        if let Some(ref target_ids) = validates {
            // First, remove this check from any nodes that currently reference it
            let all_nodes = block_on(self.state().get_nodes()).map_err(|e| e.to_string())?;
            for node in &all_nodes {
                if (node.kind == NodeKind::Task || node.kind == NodeKind::Feature)
                    && node.validated_by.contains(&id.to_string())
                {
                    // If this node is NOT in the new validates list, remove the check
                    if !target_ids.contains(&node.id) {
                        let new_vb: Vec<String> = node
                            .validated_by
                            .iter()
                            .filter(|e| e.as_str() != id)
                            .cloned()
                            .collect();
                        let update = UpdateBoardNodeRequest {
                            validated_by: Some(new_vb),
                            ..Default::default()
                        };
                        let _ = block_on(self.state().update_node(&node.id, &update));
                    }
                }
            }

            // Then, add this check to each target node's validated_by
            for target_id in target_ids {
                let task = match block_on(self.state().get_node(target_id)) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let mut new_validated_by = task.validated_by.clone();
                if !new_validated_by.contains(&id.to_string()) {
                    new_validated_by.push(id.to_string());
                }
                let update = UpdateBoardNodeRequest {
                    validated_by: Some(new_validated_by),
                    ..Default::default()
                };
                let _ = block_on(self.state().update_node(target_id, &update));
            }
        }

        Ok(json!({
            "success": true,
            "action": "updated",
            "id": id,
            "message": "Updated check."
        })
        .to_string())
    }

    /// Handle board_delete - delete a node
    fn handle_delete(&mut self, args: Value) -> Result<String, String> {
        let id = args
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("'id' required")?;

        // Get node to check kind and name
        let node = match block_on(self.state().get_node(id)) {
            Ok(n) => n,
            Err(_) => {
                return Ok(json!({
                    "success": false,
                    "message": format!("Node '{}' not found", id),
                    "reason": "not_found",
                    "hint": "Use board_view to see available task IDs"
                })
                .to_string());
            }
        };

        let node_name = node.name.clone();

        // Count references that will be cleaned up
        let refs_cleaned = self.count_references_to(id);

        // Delete the node (cascade deletes children)
        block_on(self.state().delete_node(id)).map_err(|e| e.to_string())?;

        Ok(json!({
            "success": true,
            "action": "deleted",
            "id": id,
            "refs_cleaned": refs_cleaned,
            "message": format!("Deleted '{}'. Removed from {} references.", node_name, refs_cleaned)
        })
        .to_string())
    }

    /// Handle board_requeue_node - reopen node back to pending
    fn handle_requeue_node(&self, args: &Value) -> Result<String, String> {
        let node_id = args
            .get("node_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("node_id is required")?;

        let state = self.state();
        let node = match block_on(state.get_node(node_id)) {
            Ok(n) => n,
            Err(_) => {
                return Ok(json!({
                    "success": false,
                    "message": format!("Node '{}' not found", node_id),
                    "reason": "not_found",
                    "hint": "Use board_view to see available node IDs"
                })
                .to_string());
            }
        };

        block_on(state.reopen_node(node_id)).map_err(|e| e.to_string())?;

        Ok(json!({
            "success": true,
            "action": "requeued",
            "id": node_id,
            "message": format!("Requeued '{}' back to pending.", node.name)
        })
        .to_string())
    }

    /// Count how many nodes reference this node (blocked_by or validates)
    fn count_references_to(&self, id: &str) -> usize {
        let nodes = block_on(self.state().get_nodes()).unwrap_or_default();
        nodes
            .iter()
            .filter(|n| {
                n.blocked_by.contains(&id.to_string()) || n.validates.contains(&id.to_string())
            })
            .count()
    }

    /// Walk up the parent chain from a node to find its root feature.
    /// Returns None if the node has no parent (already root) or not found.
    fn find_root_feature(&self, node_id: &str) -> Option<String> {
        let node = block_on(self.state().get_node(node_id)).ok()?;
        match node.parent_id {
            Some(ref pid) => {
                // Recurse up — if parent has a parent, keep going
                self.find_root_feature(pid).or(Some(pid.clone()))
            }
            None => {
                // This node IS a root — parent the check under it if it's a feature
                if node.kind == NodeKind::Feature {
                    Some(node.id)
                } else {
                    None
                }
            }
        }
    }
}

impl McpToolServer for BoardMcpServer {
    fn server_name(&self) -> &'static str {
        "hirsel-board-mcp"
    }

    fn tools(&self) -> Vec<Tool> {
        get_tools()
    }

    fn execute(&mut self, name: &str, args: Value) -> Result<(String, bool), String> {
        let result = match name {
            "board_routes" => self.handle_routes(),
            "board_switch_route" => self.handle_switch_route(&args),
            "board_create_route" => self.handle_create_route(&args),
            "board_set_active_route" => self.handle_set_active_route(&args),
            "board_view" => self.handle_view(),
            "board_feature" => self.handle_feature(args),
            "board_task" => self.handle_task(args),
            "board_check" => self.handle_check(args),
            "board_delete" => self.handle_delete(args),
            "board_requeue_node" => self.handle_requeue_node(&args),
            _ => Err(format!("Unknown tool: {}", name)),
        };

        result.map(|s| (s, false))
    }
}

/// Run the board MCP server for a project
///
/// Called from CLI via `hirsel __board-mcp` with HIRSEL_PROJECT_ID env var.
pub fn run_board_mcp_server(project_id: i64) -> Result<(), String> {
    let mut server = BoardMcpServer::new(project_id);
    run_mcp_server(&mut server).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_tools() {
        let tools = get_tools();
        assert_eq!(tools.len(), 10);

        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        assert!(names.contains(&"board_routes"));
        assert!(names.contains(&"board_switch_route"));
        assert!(names.contains(&"board_create_route"));
        assert!(names.contains(&"board_set_active_route"));
        assert!(names.contains(&"board_view"));
        assert!(names.contains(&"board_feature"));
        assert!(names.contains(&"board_task"));
        assert!(names.contains(&"board_check"));
        assert!(names.contains(&"board_delete"));
        assert!(names.contains(&"board_requeue_node"));
    }
}
