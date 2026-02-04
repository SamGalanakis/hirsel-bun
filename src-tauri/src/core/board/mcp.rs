//! Board MCP server for Gyp agent
//!
//! Exposes board structure manipulation via MCP tools. The agent uses these tools
//! instead of editing board.json directly, which prevents ID collisions and
//! invalid structure.
//!
//! ## Tools
//!
//! - `board_view` - View full board structure with task IDs and file paths
//! - `board_task` - Create or update a task
//! - `board_eval` - Create or update an eval
//! - `board_delete` - Delete a task or eval
//!
//! ## Content Editing
//!
//! Task/eval content lives in markdown files at `board/tasks/{id}.md`.
//! The agent edits these files directly with Read/Write tools.

use serde_json::{json, Value};
use std::future::Future;
use std::path::PathBuf;

use crate::core::delta::{DeltaExporter, DeltaState, DraftNodeTree, NodeType};
use crate::core::mcp::{run_mcp_server, McpToolServer, Tool};
use crate::core::project::ProjectStore;
use crate::core::route::{RouteFiles, RouteStore};
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
            description: "Switch to a different route. All subsequent operations will use this route.",
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
            name: "board_view",
            description: "View the full board structure with all tasks and evals. Returns JSON with task hierarchy and eval list.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        Tool {
            name: "board_task",
            description: "Create or update a task. Omit 'id' to create new. Returns the new/updated task ID and file path.",
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
            name: "board_eval",
            description: "Create or update an eval. Omit 'id' to create new. Returns the new/updated eval ID and file path.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Existing eval ID (omit for new)"
                    },
                    "name": {
                        "type": "string",
                        "description": "Eval name (required for create)"
                    },
                    "validates": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Task IDs this eval validates (required for create)"
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
            description: "Delete a task or eval by ID. Removes the node and cleans up references.",
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
    ]
}

/// Board MCP Server
pub struct BoardMcpServer {
    project_id: i64,
    route_id: Mutex<i64>,
    route_name: Mutex<String>,
}

impl BoardMcpServer {
    /// Create a new board MCP server for a project
    ///
    /// Uses the project's active route for all operations.
    pub fn new(project_id: i64) -> Self {
        // Look up the project's active route
        let (route_id, route_name) = block_on(async {
            let project_store = ProjectStore::open().await.ok()?;
            let project = project_store.get_project(project_id).await.ok()?;
            let route_id = project.active_route_id?;

            let route_store = RouteStore::new(project_id).await.ok()?;
            let route = route_store.get_route(route_id).await.ok()?;
            Some((route_id, route.name))
        })
        .unwrap_or((1, "main".to_string()));

        Self {
            project_id,
            route_id: Mutex::new(route_id),
            route_name: Mutex::new(route_name),
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

    /// Get the current route name
    fn current_route_name(&self) -> String {
        self.route_name.lock().unwrap().clone()
    }

    /// Switch to a different route
    fn switch_route(&self, route_id: i64, route_name: &str) {
        *self.route_id.lock().unwrap() = route_id;
        *self.route_name.lock().unwrap() = route_name.to_string();
    }

    /// Get the route directory (route-scoped)
    fn route_dir(&self) -> PathBuf {
        RouteFiles::routes_base_dir(self.project_id).join(self.current_route_name())
    }

    /// Get the board directory path (route-scoped)
    fn board_dir(&self) -> PathBuf {
        self.route_dir().join("board")
    }

    /// Get the tasks directory path
    fn tasks_dir(&self) -> PathBuf {
        self.board_dir().join("tasks")
    }

    /// Ensure tasks directory exists
    fn ensure_tasks_dir(&self) -> std::io::Result<PathBuf> {
        let dir = self.tasks_dir();
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(dir)
    }

    /// Get file path for a node
    fn node_file_path(&self, id: &str) -> String {
        format!("tasks/{}.md", id)
    }

    /// Write content file for a node
    fn write_content_file(&self, id: &str, content: &str) -> Result<String, String> {
        let tasks_dir = self.ensure_tasks_dir().map_err(|e| e.to_string())?;
        let file_path = tasks_dir.join(format!("{}.md", id));
        std::fs::write(&file_path, content).map_err(|e| e.to_string())?;
        Ok(self.node_file_path(id))
    }

    /// Delete content file for a node
    fn delete_content_file(&self, id: &str) -> Result<(), String> {
        let file_path = self.tasks_dir().join(format!("{}.md", id));
        if file_path.exists() {
            std::fs::remove_file(&file_path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Get all valid task IDs (for error messages)
    fn get_valid_task_ids(&self) -> Vec<String> {
        block_on(self.state().get_draft_nodes())
            .map(|nodes| {
                nodes
                    .iter()
                    .filter(|n| n.node_type == NodeType::Task)
                    .map(|n| n.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all valid node IDs (tasks and evals)
    fn get_valid_node_ids(&self) -> Vec<String> {
        block_on(self.state().get_draft_nodes())
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

        self.switch_route(route_id, &route.name);

        // Re-export for the new route
        self.sync_export()?;

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

    /// Handle board_view - returns full board structure
    fn handle_view(&self) -> Result<String, String> {
        let draft_tree = block_on(self.state().get_draft_tree()).map_err(|e| e.to_string())?;
        let all_nodes = block_on(self.state().get_draft_nodes()).map_err(|e| e.to_string())?;

        // Convert to view format - root nodes are tasks directly (no project wrapper)
        let tasks: Vec<Value> = draft_tree
            .iter()
            .filter(|n| n.node_type == NodeType::Task)
            .map(|n| self.tree_to_view_task(n))
            .collect();

        let evals: Vec<Value> = all_nodes
            .iter()
            .filter(|n| n.node_type == NodeType::Eval)
            .map(|n| {
                json!({
                    "id": n.id,
                    "name": n.name,
                    "validates": n.validates,
                    "file": self.node_file_path(&n.id)
                })
            })
            .collect();

        let task_count = self.count_tasks(&draft_tree);
        let eval_count = evals.len();

        Ok(json!({
            "tasks": tasks,
            "evals": evals,
            "summary": format!("{} tasks, {} evals", task_count, eval_count)
        })
        .to_string())
    }

    /// Convert tree node to view format (recursive)
    fn tree_to_view_task(&self, node: &DraftNodeTree) -> Value {
        let children: Vec<Value> = node
            .children
            .iter()
            .filter(|c| c.node_type == NodeType::Task)
            .map(|c| self.tree_to_view_task(c))
            .collect();

        json!({
            "id": node.id,
            "name": node.name,
            "blocked_by": node.blocked_by,
            "file": self.node_file_path(&node.id),
            "children": children
        })
    }

    /// Count all tasks in tree (recursive)
    fn count_tasks(&self, tree: &[DraftNodeTree]) -> usize {
        tree.iter()
            .filter(|n| n.node_type == NodeType::Task)
            .map(|n| 1 + self.count_task_children(n))
            .sum()
    }

    fn count_task_children(&self, node: &DraftNodeTree) -> usize {
        node.children
            .iter()
            .filter(|c| c.node_type == NodeType::Task)
            .map(|c| 1 + self.count_task_children(c))
            .sum()
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
                        "valid_task_ids": self.get_valid_task_ids()
                    })
                    .to_string());
                }
            }
        }

        if let Some(existing_id) = id {
            // Update existing task
            self.update_task(existing_id, name, blocked_by, parent_id)
        } else {
            // Create new task
            let name = name.ok_or("'name' required for new task")?;
            self.create_task(name, blocked_by, parent_id, content)
        }
    }

    /// Create a new task
    fn create_task(
        &mut self,
        name: &str,
        blocked_by: Option<Vec<String>>,
        parent_id: Option<&str>,
        content: &str,
    ) -> Result<String, String> {
        use crate::core::delta::CreateDraftNodeRequest;

        let req = CreateDraftNodeRequest {
            parent_id: parent_id.map(String::from),
            name: name.to_string(),
            node_type: NodeType::Task,
            content: content.to_string(),
            validates: vec![],
            blocked_by: blocked_by.unwrap_or_default(),
            x: None,
            y: None,
        };

        let node = block_on(self.state().create_draft_node(&req)).map_err(|e| e.to_string())?;
        let file_path = self.write_content_file(&node.id, content)?;

        // Re-export to sync board files
        self.sync_export()?;

        Ok(json!({
            "success": true,
            "action": "created",
            "id": node.id,
            "file": file_path,
            "message": format!("Created task '{}'. Edit {} to add details.", name, file_path)
        })
        .to_string())
    }

    /// Update an existing task
    fn update_task(
        &mut self,
        id: &str,
        name: Option<&str>,
        blocked_by: Option<Vec<String>>,
        parent_id: Option<&str>,
    ) -> Result<String, String> {
        use crate::core::delta::UpdateDraftNodeRequest;

        // Get existing node to check for rename
        let old_node = match block_on(self.state().get_draft_node(id)) {
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
        let req = UpdateDraftNodeRequest {
            name: name.map(String::from),
            content: None, // Content edited via files
            validates: None,
            blocked_by,
            x: None,
            y: None,
        };

        block_on(self.state().update_draft_node(id, &req)).map_err(|e| e.to_string())?;

        // Handle parent_id change if specified
        if let Some(new_parent) = parent_id {
            let new_parent = if new_parent.is_empty() {
                None
            } else {
                Some(new_parent)
            };
            block_on(self.state().move_draft_node(id, new_parent, 0)).map_err(|e| e.to_string())?;
        }

        // Re-export to sync board files
        self.sync_export()?;

        let file_path = self.node_file_path(id);

        if was_renamed {
            Ok(json!({
                "success": true,
                "action": "renamed",
                "id": id,
                "file": file_path,
                "message": format!("Renamed to '{}'. Content at {}", name.unwrap(), file_path)
            })
            .to_string())
        } else {
            Ok(json!({
                "success": true,
                "action": "updated",
                "id": id,
                "file": file_path,
                "message": format!("Updated task. Content at {}", file_path)
            })
            .to_string())
        }
    }

    /// Handle board_eval - create or update an eval
    fn handle_eval(&mut self, args: Value) -> Result<String, String> {
        let id = args.get("id").and_then(|v| v.as_str());
        let name = args.get("name").and_then(|v| v.as_str());
        let validates: Option<Vec<String>> = args.get("validates").and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        });
        let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");

        // Validate validates references exist
        if let Some(ref task_ids) = validates {
            let valid_ids = self.get_valid_task_ids();
            for task_id in task_ids {
                if !valid_ids.contains(task_id) {
                    return Ok(json!({
                        "success": false,
                        "message": format!("Cannot validate non-existent task '{}'", task_id),
                        "reason": "invalid_validates_reference",
                        "valid_task_ids": valid_ids
                    })
                    .to_string());
                }
            }
        }

        if let Some(existing_id) = id {
            // Update existing eval
            self.update_eval(existing_id, name, validates)
        } else {
            // Create new eval
            let name = name.ok_or("'name' required for new eval")?;
            let validates = validates.ok_or("'validates' required for new eval")?;
            if validates.is_empty() {
                return Err("'validates' must contain at least one task ID".to_string());
            }
            self.create_eval(name, validates, content)
        }
    }

    /// Create a new eval
    fn create_eval(
        &mut self,
        name: &str,
        validates: Vec<String>,
        content: &str,
    ) -> Result<String, String> {
        use crate::core::delta::CreateDraftNodeRequest;

        let req = CreateDraftNodeRequest {
            parent_id: None, // Evals are always at root level
            name: name.to_string(),
            node_type: NodeType::Eval,
            content: content.to_string(),
            validates: validates.clone(),
            blocked_by: vec![],
            x: None,
            y: None,
        };

        let node = block_on(self.state().create_draft_node(&req)).map_err(|e| e.to_string())?;
        let file_path = self.write_content_file(&node.id, content)?;

        // Re-export to sync board files
        self.sync_export()?;

        let validates_str = validates.join(", ");
        Ok(json!({
            "success": true,
            "action": "created",
            "id": node.id,
            "file": file_path,
            "message": format!("Created eval '{}' validating: {}", name, validates_str)
        })
        .to_string())
    }

    /// Update an existing eval
    fn update_eval(
        &mut self,
        id: &str,
        name: Option<&str>,
        validates: Option<Vec<String>>,
    ) -> Result<String, String> {
        use crate::core::delta::UpdateDraftNodeRequest;

        // Verify exists
        if block_on(self.state().get_draft_node(id)).is_err() {
            return Ok(json!({
                "success": false,
                "message": format!("Eval '{}' not found", id),
                "reason": "not_found",
                "hint": "Use board_view to see available eval IDs"
            })
            .to_string());
        }

        // Validate: cannot clear validates to empty
        if let Some(ref v) = validates {
            if v.is_empty() {
                return Err("Eval must validate at least one task".to_string());
            }
        }

        let req = UpdateDraftNodeRequest {
            name: name.map(String::from),
            content: None,
            validates,
            blocked_by: None,
            x: None,
            y: None,
        };

        block_on(self.state().update_draft_node(id, &req)).map_err(|e| e.to_string())?;

        // Re-export to sync board files
        self.sync_export()?;

        let file_path = self.node_file_path(id);
        Ok(json!({
            "success": true,
            "action": "updated",
            "id": id,
            "file": file_path,
            "message": format!("Updated eval. Content at {}", file_path)
        })
        .to_string())
    }

    /// Handle board_delete - delete a task or eval
    fn handle_delete(&mut self, args: Value) -> Result<String, String> {
        let id = args
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("'id' required")?;

        // Get node to check type and name
        let node = match block_on(self.state().get_draft_node(id)) {
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
        let file_path = self.node_file_path(id);

        // Count references that will be cleaned up
        let refs_cleaned = self.count_references_to(id);

        // Delete the node (cascade deletes children)
        block_on(self.state().delete_draft_node(id)).map_err(|e| e.to_string())?;

        // Delete content file
        self.delete_content_file(id)?;

        // Re-export to sync board files
        self.sync_export()?;

        Ok(json!({
            "success": true,
            "action": "deleted",
            "id": id,
            "file_deleted": file_path,
            "refs_cleaned": refs_cleaned,
            "message": format!("Deleted '{}'. Removed from {} references.", node_name, refs_cleaned)
        })
        .to_string())
    }

    /// Count how many nodes reference this node (blocked_by or validates)
    fn count_references_to(&self, id: &str) -> usize {
        let nodes = block_on(self.state().get_draft_nodes()).unwrap_or_default();
        nodes
            .iter()
            .filter(|n| {
                n.blocked_by.contains(&id.to_string()) || n.validates.contains(&id.to_string())
            })
            .count()
    }

    /// Re-export board to sync files
    fn sync_export(&self) -> Result<(), String> {
        let mut exporter = DeltaExporter::with_route(self.project_id, self.current_route_id());
        exporter.export_for_agent().map_err(|e| e.to_string())?;
        Ok(())
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
            "board_view" => self.handle_view(),
            "board_task" => self.handle_task(args),
            "board_eval" => self.handle_eval(args),
            "board_delete" => self.handle_delete(args),
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
        assert_eq!(tools.len(), 6);

        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        assert!(names.contains(&"board_routes"));
        assert!(names.contains(&"board_switch_route"));
        assert!(names.contains(&"board_view"));
        assert!(names.contains(&"board_task"));
        assert!(names.contains(&"board_eval"));
        assert!(names.contains(&"board_delete"));
    }
}
