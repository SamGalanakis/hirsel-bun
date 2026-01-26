//! Board Gyp Context Builder
//!
//! Builds system prompts and invocation context for Gyp when editing
//! the SpecFlow board. Uses a Tasks + Evals model:
//! - Tasks: Nested tree of work items (post-it style)
//! - Evals: Flat list of verifications that validate tasks
//!
//! ## File Storage Model
//!
//! Each top-level task is stored as a separate JSON file:
//! `~/.hirsel/projects/{project_id}/board/{task-slug}.json`

use std::path::PathBuf;

use crate::core::config::hirsel_dir;
use crate::core::gyp_chat::{GypChatMessage, GypChatResult, GypChatStore};

/// Scope for board context operations
#[derive(Debug, Clone)]
pub enum BoardContextScope {
    /// Whole board - agent can see all task files
    WholeBoard,
    /// Focused on a specific task tree
    FocusedTask { task_id: String, task_name: String },
}

impl Default for BoardContextScope {
    fn default() -> Self {
        Self::WholeBoard
    }
}

/// Context builder for board-related Gyp interactions
pub struct BoardGypContext {
    project_id: i64,
    board_dir: PathBuf,
    scope: BoardContextScope,
}

impl BoardGypContext {
    /// Create a new board context for a project (whole board scope)
    pub fn new(project_id: i64) -> Self {
        let board_dir = hirsel_dir()
            .join("projects")
            .join(project_id.to_string())
            .join("board");

        Self {
            project_id,
            board_dir,
            scope: BoardContextScope::WholeBoard,
        }
    }

    /// Create a board context focused on a specific task
    pub fn with_focus(project_id: i64, task_id: String, task_name: String) -> Self {
        let board_dir = hirsel_dir()
            .join("projects")
            .join(project_id.to_string())
            .join("board");

        Self {
            project_id,
            board_dir,
            scope: BoardContextScope::FocusedTask { task_id, task_name },
        }
    }

    /// Get the project ID
    pub fn project_id(&self) -> i64 {
        self.project_id
    }

    /// Get the board directory path
    pub fn board_dir(&self) -> &PathBuf {
        &self.board_dir
    }

    /// Get the current scope
    pub fn scope(&self) -> &BoardContextScope {
        &self.scope
    }

    /// Build the system prompt based on scope
    pub fn build_system_prompt(&self) -> String {
        match &self.scope {
            BoardContextScope::WholeBoard => self.build_whole_board_prompt(),
            BoardContextScope::FocusedTask { task_id, task_name } => {
                self.build_focused_task_prompt(task_id, task_name)
            }
        }
    }

    /// Build system prompt for whole board operations
    fn build_whole_board_prompt(&self) -> String {
        format!(
            r#"You are Gyp, an AI assistant helping edit a SpecFlow board for project planning.

## Board Location

Board directory: {board_dir}/
Each top-level task has its own file: `{{task-slug}}.json`

List files in the directory to see all available tasks.

## Data Model

The board has two types of entities:

### Tasks (Nested Tree)
Work items organized in a tree structure. Each task has:
- **id**: Slug identifier (e.g., "build-api") - auto-generated from name
- **name**: Display name
- **status**: todo | doing | done | blocked
- **content**: Freeform description/notes
- **children**: Nested child tasks
- **x, y**: Optional position on canvas

### Evals (Flat List)
Verifications that reference and validate tasks. Each eval has:
- **id**: Slug identifier (e.g., "api-test")
- **name**: Display name
- **status**: blocked | queued | in_progress | passed | failed
- **content**: What to verify
- **validates**: Array of task IDs this eval validates
- **x, y**: Optional position on canvas

## Validation Rules

A task is considered "validated" when:
1. It has at least one eval with status="passed" that lists it in validates[], OR
2. ALL of its children are validated (recursive)

Validation propagates up the tree automatically.

## File Format

Each task file contains a single top-level task and its related evals:

```json
{{
  "task": {{
    "id": "build-api",
    "name": "Build API",
    "status": "doing",
    "content": "Implement REST API with user endpoints",
    "children": [
      {{
        "id": "user-endpoints",
        "name": "User Endpoints",
        "status": "done",
        "content": "CRUD operations for users",
        "children": []
      }}
    ]
  }},
  "evals": [
    {{
      "id": "api-integration-test",
      "name": "API Integration Test",
      "status": "passed",
      "content": "Run the test suite against deployed API",
      "validates": ["user-endpoints"]
    }}
  ]
}}
```

## Status Values

**Task status:**
- `todo` - Not started
- `doing` - In progress
- `done` - Work completed
- `blocked` - Cannot proceed

**Eval status:**
- `blocked` - Dependencies not ready
- `queued` - Ready to run
- `in_progress` - Currently running
- `passed` - Verification succeeded
- `failed` - Verification failed

## Multi-File Workflow

1. List files in the board directory to see all top-level tasks
2. Read specific task files to understand their content
3. Modify task files using the Write tool (complete file write)
4. To add a new top-level task, create a new file with the task slug as filename
5. To delete a task, delete its file

## Common Operations

### Add a top-level task
Create a new file `{board_dir}/{{task-slug}}.json` with the task structure.

### Add a subtask
Read the parent task's file, add to its children array, write back.

### Add an eval
Read the relevant task file, add to its evals array, write back.
Note: If an eval validates tasks across multiple files, add it to each file.

### Mark progress
Update task status: todo -> doing -> done
Update eval status: blocked -> queued -> in_progress -> passed/failed"#,
            board_dir = self.board_dir.display()
        )
    }

    /// Build system prompt for focused task operations
    fn build_focused_task_prompt(&self, task_id: &str, task_name: &str) -> String {
        format!(
            r#"You are Gyp, an AI assistant helping edit a specific task on a SpecFlow board.

## Focused Task

You are working on: **{task_name}** (id: {task_id})
Task file: {board_dir}/{task_id}.json

## Data Model

### Task Structure
Each task has:
- **id**: Slug identifier (e.g., "build-api")
- **name**: Display name
- **status**: todo | doing | done | blocked
- **content**: Freeform description/notes
- **children**: Nested child tasks
- **x, y**: Optional position on canvas

### Evals
Verifications that validate tasks. Each eval has:
- **id**: Slug identifier (e.g., "api-test")
- **name**: Display name
- **status**: blocked | queued | in_progress | passed | failed
- **content**: What to verify
- **validates**: Array of task IDs this eval validates

## File Format

```json
{{
  "task": {{
    "id": "{task_id}",
    "name": "{task_name}",
    "status": "doing",
    "content": "...",
    "children": [...]
  }},
  "evals": [...]
}}
```

## Workflow

1. Read `{board_dir}/{task_id}.json` to understand the current state
2. Make changes to the task tree or evals as requested
3. Write the complete file back using the Write tool

## Common Operations

### Add a subtask
Add a new entry to the task's `children` array.

### Break down task
Convert a leaf task into a parent by adding children.

### Add an eval
Add to the `evals` array with validates[] pointing to task IDs.

### Update status
Change task status: todo -> doing -> done
Change eval status: blocked -> queued -> in_progress -> passed/failed

### Edit content
Update the `content` field of the task or any child."#,
            task_name = task_name,
            task_id = task_id,
            board_dir = self.board_dir.display()
        )
    }

    /// Build invocation context for a specific task
    pub fn build_invocation_context(
        &self,
        task_id: &str,
        task_name: &str,
        user_message: &str,
    ) -> String {
        format!(
            r#"You were invoked from task "{task_name}" (id: {task_id}).
File: {board_dir}/{task_id}.json

User request: {user_message}"#,
            task_name = task_name,
            task_id = task_id,
            board_dir = self.board_dir.display(),
            user_message = user_message
        )
    }

    /// Build context for general board operations (no specific task)
    pub fn build_general_context(&self, user_message: &str) -> String {
        format!(
            r#"User request about the board: {user_message}

Board directory: {board_dir}/
List files to see all task files, then read specific files as needed."#,
            user_message = user_message,
            board_dir = self.board_dir.display()
        )
    }

    /// Get recent board chat history for context continuity
    pub fn get_recent_history(&self, limit: usize) -> GypChatResult<Vec<GypChatMessage>> {
        let store = GypChatStore::open()?;
        store.get_board_messages(self.project_id, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_builder_whole_board() {
        let ctx = BoardGypContext::new(42);
        assert_eq!(ctx.project_id(), 42);

        let system_prompt = ctx.build_system_prompt();
        assert!(system_prompt.contains("SpecFlow board"));
        assert!(system_prompt.contains("Tasks (Nested Tree)"));
        assert!(system_prompt.contains("Evals"));
        assert!(system_prompt.contains("Multi-File Workflow"));
        assert!(system_prompt.contains("{task-slug}.json"));
    }

    #[test]
    fn test_context_builder_focused() {
        let ctx = BoardGypContext::with_focus(42, "build-api".to_string(), "Build API".to_string());

        let system_prompt = ctx.build_system_prompt();
        assert!(system_prompt.contains("Build API"));
        assert!(system_prompt.contains("build-api"));
        assert!(system_prompt.contains("Focused Task"));
    }

    #[test]
    fn test_invocation_context() {
        let ctx = BoardGypContext::new(42);
        let invocation = ctx.build_invocation_context("build-api", "Build API", "Add a child");
        assert!(invocation.contains("Build API"));
        assert!(invocation.contains("build-api"));
        assert!(invocation.contains("Add a child"));
        assert!(invocation.contains(".json"));
    }
}
