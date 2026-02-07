//! Unified Gyp Context Builder
//!
//! Single source of truth for all Gyp session configuration:
//! - System prompts
//! - Working directories
//! - MCP server configuration
//! - Message context injection

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::config::hirsel_dir;
use crate::core::draft::{StartingPoint, WorkspaceProvider};

/// Scope for Gyp session - determines prompt, working dir, and MCP config
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum GypScope {
    /// General chat - no project context
    #[serde(rename = "general")]
    General,
    /// Run context - working on an active run
    #[serde(rename = "run")]
    Run {
        #[serde(rename = "runName")]
        run_name: String,
        /// Resolved workspace path (from WorkspaceProvider)
        #[serde(rename = "workspacePath", default)]
        workspace_path: PathBuf,
        /// Original project path (for context in prompt)
        #[serde(rename = "projectPath", default)]
        project_path: Option<PathBuf>,
        /// Project ID for history scoping
        #[serde(rename = "projectId", default)]
        project_id: Option<i64>,
    },
    /// Board context - planning on SpecFlow board
    #[serde(rename = "board")]
    Board {
        #[serde(rename = "projectId")]
        project_id: i64,
        /// Project workspace path (from StartingPoint)
        #[serde(rename = "workspacePath", default)]
        workspace_path: Option<PathBuf>,
        /// Focused task (optional)
        #[serde(default)]
        focus: Option<TaskFocus>,
    },
}

/// Focused task for board context
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskFocus {
    pub task_id: String,
    pub task_name: String,
}

/// Built context for a Gyp session
#[derive(Debug, Clone)]
pub struct GypSessionConfig {
    /// System prompt for the session
    pub system_prompt: String,
    /// Working directory for the agent
    pub working_dir: PathBuf,
    /// MCP server configurations
    pub mcp_servers: Vec<McpServerConfig>,
    /// Scope identifier for history storage
    pub history_scope: HistoryScope,
}

/// MCP server configuration
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub command: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Scope for chat history storage
#[derive(Debug, Clone)]
pub struct HistoryScope {
    pub project_id: Option<i64>,
    pub run_name: Option<String>,
}

/// Unified Gyp context builder
pub struct GypContextBuilder {
    scope: GypScope,
}

impl GypContextBuilder {
    /// Create builder for general chat (no project context)
    pub fn general() -> Self {
        Self {
            scope: GypScope::General,
        }
    }

    /// Create builder for run context
    pub fn for_run(run_name: &str, workspace: &dyn WorkspaceProvider) -> Self {
        Self {
            scope: GypScope::Run {
                run_name: run_name.to_string(),
                workspace_path: workspace.workspace_path(run_name),
                project_path: None,
                project_id: None,
            },
        }
    }

    /// Create builder for run context with project info
    pub fn for_run_with_project(
        run_name: &str,
        workspace: &dyn WorkspaceProvider,
        project_path: Option<PathBuf>,
        project_id: Option<i64>,
    ) -> Self {
        Self {
            scope: GypScope::Run {
                run_name: run_name.to_string(),
                workspace_path: workspace.workspace_path(run_name),
                project_path,
                project_id,
            },
        }
    }

    /// Create builder for board context
    pub fn for_board(project_id: i64, starting_point: &StartingPoint) -> Self {
        Self {
            scope: GypScope::Board {
                project_id,
                workspace_path: starting_point.local_path(),
                focus: None,
            },
        }
    }

    /// Create builder for board context with task focus
    pub fn for_board_focused(
        project_id: i64,
        starting_point: &StartingPoint,
        task_id: String,
        task_name: String,
    ) -> Self {
        Self {
            scope: GypScope::Board {
                project_id,
                workspace_path: starting_point.local_path(),
                focus: Some(TaskFocus { task_id, task_name }),
            },
        }
    }

    /// Build the complete session configuration
    pub fn build(&self) -> GypSessionConfig {
        GypSessionConfig {
            system_prompt: self.build_system_prompt(),
            working_dir: self.resolve_working_dir(),
            mcp_servers: self.build_mcp_servers(),
            history_scope: self.build_history_scope(),
        }
    }

    /// Get the scope (for serialization to frontend)
    pub fn scope(&self) -> &GypScope {
        &self.scope
    }

    // =========================================================================
    // System Prompt Building
    // =========================================================================

    fn build_system_prompt(&self) -> String {
        let mut sections = vec![self.intro_section()];

        match &self.scope {
            GypScope::General => {
                sections.push(self.general_scope_section());
            }
            GypScope::Run {
                run_name,
                workspace_path,
                project_path,
                ..
            } => {
                sections.push(self.run_scope_section(run_name, workspace_path, project_path));
                sections.push(self.run_tools_section());
            }
            GypScope::Board {
                project_id,
                workspace_path,
                focus,
            } => {
                sections.push(self.board_scope_section(*project_id, workspace_path, focus));
                sections.push(self.board_data_model_section());
                sections.push(self.board_workflow_section(*project_id));
            }
        }

        sections.push(self.rules_section());
        sections.join("\n\n")
    }

    fn intro_section(&self) -> String {
        "You are Gyp, an AI assistant for Hirsel - a tool for managing AI-driven development runs."
            .to_string()
    }

    fn general_scope_section(&self) -> String {
        r#"## Current Scope: General

You're in general chat mode with no specific project context.
You can help with questions about Hirsel, general coding tasks, or anything else."#
            .to_string()
    }

    fn run_scope_section(
        &self,
        run_name: &str,
        workspace_path: &PathBuf,
        project_path: &Option<PathBuf>,
    ) -> String {
        let project_info = project_path
            .as_ref()
            .map(|p| format!("\nOriginal project: `{}`", p.display()))
            .unwrap_or_default();

        format!(
            r#"## Current Scope: Run

You're working on run: **{run_name}**
Workspace: `{workspace}`{project_info}

The workspace contains the run's working copy of the codebase.
Use the hirsel MCP tools to manage tasks and communicate with workers."#,
            run_name = run_name,
            workspace = workspace_path.display(),
            project_info = project_info
        )
    }

    fn run_tools_section(&self) -> String {
        r#"## Available Tools

Use these hirsel MCP tools (NOT CLI commands):
- `task_list` - List all tasks in the run
- `task_add` - Add a new task
- `task_done` - Mark a task as complete
- `msg_send` - Send message to workers
- `msg_read` - Read messages from workers

For feature/check editing, use Read/Edit/Write tools on files in the workspace."#
            .to_string()
    }

    fn board_scope_section(
        &self,
        project_id: i64,
        workspace_path: &Option<PathBuf>,
        focus: &Option<TaskFocus>,
    ) -> String {
        let board_dir = hirsel_dir()
            .join("projects")
            .join(project_id.to_string())
            .join("board");

        let focus_section = match focus {
            Some(f) => format!(
                r#"
### Focused Task

You are working on: **{}** (id: `{}`)"#,
                f.task_name, f.task_id
            ),
            None => String::new(),
        };

        let workspace_section = match workspace_path {
            Some(path) => format!(
                r#"

### Project Workspace

**IMPORTANT:** The actual project codebase is at: `{}`

When working on tasks related to actual code:
- Read/write code files from the project workspace
- DO NOT confuse board files with project source code
- The board directory contains only planning data"#,
                path.display()
            ),
            None => String::new(),
        };

        format!(
            r#"## Current Scope: SpecFlow Board

You're helping plan work on a SpecFlow board (project ID: {project_id}).
Board directory: `{board_dir}`
Content files: `{board_dir}/tasks/{{id}}.md`
{focus_section}{workspace_section}"#,
            project_id = project_id,
            board_dir = board_dir.display(),
            focus_section = focus_section,
            workspace_section = workspace_section
        )
    }

    fn board_data_model_section(&self) -> String {
        r#"## Board Tools (MCP)

Use these hirsel MCP tools to manage board structure:

### Route Management

**`board_routes`** - List all routes for this project
- Shows route hierarchy (which routes forked from which)
- Returns: id, name, created_at, parent_route_id, forked_from_version, active flag
- Use to understand available exploration branches

**`board_switch_route`** - Switch to a different route
- `{ route_id }` → all subsequent operations use this route
- Use when user wants to explore a different branch
- Each route has its own independent board state

### Board Structure

**`board_view`** - View the full board structure
- No parameters. Returns JSON with all features, tasks, and checks.

**`board_feature`** - Create or update a feature (high-level goal)
- Create: `{ name, blocked_by?, validated_by?, parent_id?, content? }` → returns new ID and file path
- Update: `{ id, name?, blocked_by?, validated_by?, parent_id? }`
- Features are dispatch units. On dispatch, each feature gets a plan worker that decomposes it.
- Use for broad objectives: "Add OAuth authentication", "Refactor database layer"

**`board_task`** - Create or update an implementation task
- Create: `{ name, blocked_by?, parent_id?, validated_by?, content? }` → returns new ID and file path
- Update: `{ id, name?, blocked_by?, parent_id?, validated_by? }`
- Tasks skip planning — dispatched directly to workers.
- Use for specific, detailed changes the user has fully specified.

**`board_check`** - Create or update a check (validation)
- Create: `{ name, validates?, content? }` → returns new ID and file path
- Update: `{ id, validates? }`
- `validates` is convenience sugar — writes `validated_by` on each referenced feature/task. Optional for global/e2e checks.

**`board_delete`** - Delete a node
- `{ id }` → removes node, deletes file, cleans up references. Root nodes cannot be deleted.

## Content Editing

Node content lives in markdown files:
- Location: `board/tasks/{id}.md`
- Edit these files directly with Read/Write tools
- Changes sync automatically

## Dependencies

**`blocked_by`:** Nodes that must complete before this node can start
- Example: `["setup-db", "config-env"]` means this node waits for both
- If a blocker has `validated_by` checks, downstream waits for Validated (all checks pass)
- If a blocker has no `validated_by`, downstream waits for Done

**`validated_by`:** (Features and tasks) Check IDs that validate this node
- Example: `["test-auth"]` means the node is Validated only when that check passes
- Multiple checks: node is Validated only when ALL pass

**Two types of checks:**
- **Targeted checks** — validate specific features or tasks. Set `validated_by` on targets pointing to these checks, or use `validates` on the check as sugar. Check runs when its target nodes are done.
- **Global/E2E checks** — project-level validation. Do NOT list in any node's `validated_by`. Schedule via `blocked_by`. Do not gate individual node status.

**Key:** Top-level project checks (e2e tests, integration tests) should NOT be listed in any node's `validated_by`. They run independently via `blocked_by` and validate the project holistically.

## Routes (Parallel Exploration)

Routes allow forking the board to explore different approaches:
- Each route is an independent copy of the board state
- The "main" route is the default starting point
- Forked routes inherit nodes from their parent at fork time
- Use `board_routes` to see available routes, `board_switch_route` to change

## When to Use Features vs Tasks

**Default to `board_feature`** for top-level items. Features get plan workers that:
- Read the codebase to understand what exists
- Decompose features into implementation tasks
- Set up dependencies between tasks
- Create check criteria

**Use `board_task`** only when:
- The user has given very specific implementation instructions
- You're adding a small, targeted change (e.g., "fix typo in README")
- The user explicitly asks for direct tasks instead of features

## Workflow

1. Call `board_view` to see current board structure
2. Use `board_feature` for goals, `board_task` for specific changes, `board_check` for validation
3. Edit `board/tasks/{id}.md` files for detailed content
4. (Optional) Use routes to explore alternatives without losing work

**IMPORTANT:** There is NO board.json file. Structure is managed ONLY via MCP tools."#
            .to_string()
    }

    fn board_workflow_section(&self, _project_id: i64) -> String {
        // Workflow is now documented in board_data_model_section
        String::new()
    }

    fn rules_section(&self) -> String {
        let scope_rules = match &self.scope {
            GypScope::General => "",
            GypScope::Run { .. } => "\n- Use hirsel MCP tools, NOT CLI commands",
            GypScope::Board { .. } => {
                r#"
- Use hirsel MCP tools for board structure (board_view, board_feature, board_task, board_check, board_delete)
- Use board_routes/board_switch_route for route management
- Edit content files directly at board/tasks/{id}.md
- DON'T list tasks/checks in chat - the user sees them in the board visualization
- After editing, just confirm briefly (e.g., "Done. Added 10 tasks and 5 checks.")
- Always parent tasks and checks under a feature so they appear connected in the tree. Only omit parent_id for truly global/cross-feature checks."#
            }
        };

        format!(
            r#"## Rules

- Be concise and direct
- Read files before editing them{scope_rules}"#,
            scope_rules = scope_rules
        )
    }

    // =========================================================================
    // Working Directory Resolution
    // =========================================================================

    fn resolve_working_dir(&self) -> PathBuf {
        match &self.scope {
            GypScope::General => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            GypScope::Run { workspace_path, .. } => workspace_path.clone(),
            GypScope::Board {
                project_id,
                workspace_path,
                ..
            } => {
                // Prefer project workspace so Claude CLI picks up CLAUDE.md
                // Fall back to board directory (route-scoped)
                workspace_path.clone().unwrap_or_else(|| {
                    // Get route-scoped board directory
                    let board_dir = Self::get_board_dir(*project_id);
                    // Ensure directory exists
                    if !board_dir.exists() {
                        let _ = std::fs::create_dir_all(&board_dir);
                    }
                    board_dir
                })
            }
        }
    }

    /// Get the board directory for a project's active route
    fn get_board_dir(project_id: i64) -> PathBuf {
        use crate::core::route::RouteFiles;

        // Try to get active route name, fallback to "main"
        let route_name =
            Self::get_active_route_name(project_id).unwrap_or_else(|| "main".to_string());
        RouteFiles::new(project_id, &route_name).board_dir()
    }

    /// Get the active route name for a project
    fn get_active_route_name(project_id: i64) -> Option<String> {
        use crate::core::project::ProjectStore;
        use crate::core::route::RouteStore;

        // Use block_on since this is called from sync context
        let rt = tokio::runtime::Handle::try_current().ok()?;
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                let project_store = ProjectStore::open().await.ok()?;
                let project = project_store.get_project(project_id).await.ok()?;
                let route_id = project.active_route_id?;

                let route_store = RouteStore::new(project_id).await.ok()?;
                let route = route_store.get_route(route_id).await.ok()?;
                Some(route.name)
            })
        })
    }

    // =========================================================================
    // MCP Server Configuration
    // =========================================================================

    fn build_mcp_servers(&self) -> Vec<McpServerConfig> {
        match &self.scope {
            GypScope::General => vec![],
            GypScope::Run { run_name, .. } => {
                vec![McpServerConfig {
                    name: "hirsel".to_string(),
                    command: vec!["hirsel".to_string(), "__worker-mcp".to_string()],
                    env: vec![
                        ("HIRSEL_RUN".to_string(), run_name.clone()),
                        ("HIRSEL_WORKER".to_string(), "gyp".to_string()),
                    ],
                }]
            }
            GypScope::Board { project_id, .. } => {
                // Board context uses MCP tools for structure manipulation
                vec![McpServerConfig {
                    name: "hirsel".to_string(),
                    command: vec!["hirsel".to_string(), "__board-mcp".to_string()],
                    env: vec![("HIRSEL_PROJECT_ID".to_string(), project_id.to_string())],
                }]
            }
        }
    }

    // =========================================================================
    // History Scope
    // =========================================================================

    fn build_history_scope(&self) -> HistoryScope {
        match &self.scope {
            GypScope::General => HistoryScope {
                project_id: None,
                run_name: None,
            },
            GypScope::Run {
                run_name,
                project_id,
                ..
            } => HistoryScope {
                project_id: *project_id,
                run_name: Some(run_name.clone()),
            },
            GypScope::Board { project_id, .. } => HistoryScope {
                project_id: Some(*project_id),
                run_name: None,
            },
        }
    }

    // =========================================================================
    // Message Context (for injecting into user messages)
    // =========================================================================

    /// Build context to prepend to a user message
    pub fn build_message_context(&self, user_message: &str) -> String {
        match &self.scope {
            GypScope::General => user_message.to_string(),
            GypScope::Run { .. } => {
                // Run context is in system prompt, no per-message injection needed
                user_message.to_string()
            }
            GypScope::Board { focus, .. } => match focus {
                Some(f) => format!(
                    "[Working on task \"{}\" (id: {})]\n\n{}",
                    f.task_name, f.task_id, user_message
                ),
                None => user_message.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_general_scope() {
        let builder = GypContextBuilder::general();
        let config = builder.build();

        assert!(config.system_prompt.contains("General"));
        assert!(config.mcp_servers.is_empty());
        assert!(config.history_scope.project_id.is_none());
        assert!(config.history_scope.run_name.is_none());
    }

    #[test]
    fn test_board_scope() {
        let starting_point = StartingPoint::LocalFolder {
            path: "/home/user/myproject".to_string(),
        };
        let builder = GypContextBuilder::for_board(42, &starting_point);
        let config = builder.build();

        assert!(config.system_prompt.contains("SpecFlow Board"));
        assert!(config.system_prompt.contains("/home/user/myproject"));
        assert!(config.system_prompt.contains("project ID: 42"));
        assert!(config.system_prompt.contains("MCP tools"));
        // Concise data model summary
        assert!(config.system_prompt.contains("tasks"));
        assert!(config.system_prompt.contains("checks"));
        assert_eq!(config.working_dir, PathBuf::from("/home/user/myproject"));
        assert_eq!(config.history_scope.project_id, Some(42));
    }

    #[test]
    fn test_board_scope_focused() {
        let starting_point = StartingPoint::Greenfield;
        let builder = GypContextBuilder::for_board_focused(
            42,
            &starting_point,
            "build-api".to_string(),
            "Build API".to_string(),
        );
        let config = builder.build();

        assert!(config.system_prompt.contains("Focused Task"));
        assert!(config.system_prompt.contains("Build API"));
        assert!(config.system_prompt.contains("build-api"));
    }

    #[test]
    fn test_message_context_board_focused() {
        let starting_point = StartingPoint::Greenfield;
        let builder = GypContextBuilder::for_board_focused(
            42,
            &starting_point,
            "build-api".to_string(),
            "Build API".to_string(),
        );

        let msg = builder.build_message_context("Add a subtask");
        assert!(msg.contains("Working on task"));
        assert!(msg.contains("Build API"));
        assert!(msg.contains("Add a subtask"));
    }
}
