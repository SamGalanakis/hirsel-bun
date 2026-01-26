//! Board Gyp Context Builder
//!
//! Builds system prompts and invocation context for Gyp when editing
//! the SpecFlow board. Uses a Tasks + Evals model:
//! - Tasks: Nested tree of work items (post-it style)
//! - Evals: Flat list of verifications that validate tasks

use std::path::PathBuf;

use crate::core::config::hirsel_dir;
use crate::core::gyp_chat::{GypChatMessage, GypChatResult, GypChatStore};

/// Context builder for board-related Gyp interactions
pub struct BoardGypContext {
    project_id: i64,
    board_dir: PathBuf,
}

impl BoardGypContext {
    /// Create a new board context for a project
    pub fn new(project_id: i64) -> Self {
        let board_dir = hirsel_dir()
            .join("projects")
            .join(project_id.to_string())
            .join("board");

        Self {
            project_id,
            board_dir,
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

    /// Build the system prompt for board editing
    pub fn build_system_prompt(&self) -> String {
        format!(
            r#"You are Gyp, an AI assistant helping edit a SpecFlow board for project planning.

## Board Location
Board file: {board_dir}/board.json

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

Validation propagates up the tree automatically. When specifying validates[], only list the
direct leaf tasks being verified - do NOT include parent tasks. Parents are validated
automatically when all their children are validated.

## File Format (Version 2)

```json
{{
  "version": 2,
  "tasks": [
    {{
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
        }},
        {{
          "id": "auth-endpoints",
          "name": "Auth Endpoints",
          "status": "todo",
          "content": "Login/logout/refresh",
          "children": []
        }}
      ]
    }}
  ],
  "evals": [
    {{
      "id": "api-integration-test",
      "name": "API Integration Test",
      "status": "passed",
      "content": "Run the test suite against deployed API",
      "validates": ["user-endpoints"]
    }},
    {{
      "id": "auth-e2e-test",
      "name": "Auth E2E Test",
      "status": "blocked",
      "content": "Test full auth flow",
      "validates": ["auth-endpoints", "build-api"]
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

## Rules

1. Read board.json to understand current state
2. Use the Write tool to make changes (complete file write)
3. Maintain valid JSON structure with version: 2
4. Use slug IDs (lowercase, hyphenated) - derive from name
5. Keep existing IDs stable when updating
6. Evals reference tasks by ID in the validates[] array
7. An eval can validate multiple tasks
8. A task can be validated by multiple evals

## Common Operations

### Add a task
Add to the `tasks` array (for root tasks) or a task's `children` array.
Use a slug ID derived from the name.

### Break down a task
Add child tasks to an existing task's children array.

### Add an eval
Add to the `evals` array. Set validates[] to reference the task IDs it will verify.

### Connect eval to tasks
Update an eval's validates[] array to include task IDs.

### Mark progress
Update task status: todo -> doing -> done
Update eval status: blocked -> queued -> in_progress -> passed/failed

### Reorganize
Move tasks by editing their position in the tree structure.
Move evals by updating their x,y positions."#,
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
File: {board_dir}/board.json

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

Board file: {board_dir}/board.json
Use the Read tool to read the board state."#,
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
    fn test_context_builder() {
        let ctx = BoardGypContext::new(42);
        assert_eq!(ctx.project_id(), 42);

        let system_prompt = ctx.build_system_prompt();
        assert!(system_prompt.contains("SpecFlow board"));
        assert!(system_prompt.contains("Tasks (Nested Tree)"));
        assert!(system_prompt.contains("Evals (Flat List)"));
        assert!(system_prompt.contains("board.json"));
        assert!(system_prompt.contains("version: 2"));

        let invocation = ctx.build_invocation_context("build-api", "Build API", "Add a child");
        assert!(invocation.contains("Build API"));
        assert!(invocation.contains("build-api"));
        assert!(invocation.contains("Add a child"));
    }
}
