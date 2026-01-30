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
            },
        }
    }

    /// Create builder for run context with project path
    pub fn for_run_with_project(
        run_name: &str,
        workspace: &dyn WorkspaceProvider,
        project_path: Option<PathBuf>,
    ) -> Self {
        Self {
            scope: GypScope::Run {
                run_name: run_name.to_string(),
                workspace_path: workspace.workspace_path(run_name),
                project_path,
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

For spec/eval editing, use Read/Edit/Write tools on files in the workspace."#
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
- DO NOT confuse board JSON files with project source code
- The board directory contains only planning data"#,
                path.display()
            ),
            None => String::new(),
        };

        format!(
            r#"## Current Scope: SpecFlow Board

You're helping plan work on a SpecFlow board (project ID: {project_id}).
Board file: `{board_dir}/board.json`
{focus_section}{workspace_section}"#,
            project_id = project_id,
            board_dir = board_dir.display(),
            focus_section = focus_section,
            workspace_section = workspace_section
        )
    }

    fn board_data_model_section(&self) -> String {
        r#"## Board Data Model

The board is stored in `board.json` with two sections:

### Tasks (nested tree)
```json
{
  "tasks": [
    {
      "id": "feature-name",
      "name": "Feature Name",
      "content": "Description of what to build...",
      "children": [
        { "id": "subtask-1", "name": "Subtask 1", "content": "...", "children": [] }
      ]
    }
  ]
}
```

**IMPORTANT:** The project itself is the implicit root - it is NOT in this file.
All top-level items in the `tasks` array are direct children of the project.
Break work into granular tasks. Each task should be a single, focused unit of work.

### Evals (flat list of verifications)
```json
{
  "evals": [
    {
      "id": "api-works",
      "name": "API Returns Valid Data",
      "content": "Steps to verify this requirement...",
      "validates": ["endpoint-get", "endpoint-post"]
    }
  ]
}
```

The `validates` array specifies which tasks this eval verifies:
- **Specific task IDs**: Eval runs after those tasks complete, verifies their work
- **Empty array `[]`**: Final project-level gate - runs after ALL tasks complete

### ID Format
IDs must be lowercase-hyphenated slugs (e.g., `build-api`, `user-auth-flow`).
IDs must be unique across all tasks and evals."#
            .to_string()
    }

    fn board_workflow_section(&self, _project_id: i64) -> String {
        r#"## Editing

Read board.json first, then write complete file back after changes."#
            .to_string()
    }

    fn rules_section(&self) -> String {
        let scope_rules = match &self.scope {
            GypScope::General => "",
            GypScope::Run { .. } => "\n- Use hirsel MCP tools, NOT CLI commands",
            GypScope::Board { .. } => {
                r#"
- Edit board files with Write tool (complete rewrites)
- DON'T list tasks/evals in chat - the user sees them in the board visualization
- After editing, just confirm briefly (e.g., "Done. Added 10 tasks and 5 evals.")"#
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
                // Fall back to board directory
                workspace_path.clone().unwrap_or_else(|| {
                    hirsel_dir()
                        .join("projects")
                        .join(project_id.to_string())
                        .join("board")
                })
            }
        }
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
            GypScope::Board { .. } => {
                // Board context doesn't need MCP tools currently
                // Could add board-specific MCP in future
                vec![]
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
            GypScope::Run { run_name, .. } => HistoryScope {
                project_id: None, // TODO: Could add project_id to Run scope
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
        assert!(config.system_prompt.contains("board.json"));
        // Concise data model summary
        assert!(config.system_prompt.contains("tasks"));
        assert!(config.system_prompt.contains("evals"));
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
