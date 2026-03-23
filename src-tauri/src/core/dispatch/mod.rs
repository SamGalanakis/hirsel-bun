//! Dispatch Service - creates runs from board task nodes
//!
//! This module handles dispatching runs from the SpecFlow board:
//! - Getting dispatch scope (task + descendants + validating evals)
//! - Creating board snapshots at dispatch time
//! - Generating spec.md and eval.md from board content
//! - Recording dispatch in task_runs junction table

use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::PathBuf;
use tracing::info;

use crate::core::board::{BoardService, BoardSnapshot, DispatchPreview, Eval, TaskTree};
use crate::core::state::StateError;

/// Block on an async future in a sync context
fn block_on<F: Future>(f: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(f)),
        Err(_) => tokio::runtime::Runtime::new()
            .expect("Failed to create tokio runtime")
            .block_on(f),
    }
}

/// Error type for dispatch operations
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("Board error: {0}")]
    Board(#[from] crate::core::board::BoardError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Task not found: {0}")]
    TaskNotFound(String),
    #[error("Project not found: {0}")]
    ProjectNotFound(i64),
    #[error("Git error: {0}")]
    Git(String),
    #[error("State error: {0}")]
    State(#[from] StateError),
}

pub type DispatchResult<T> = Result<T, DispatchError>;

/// Configuration for dispatching a run
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct DispatchConfig {
    /// Custom run name (auto-generated if not provided)
    pub runtime_name: Option<String>,
    /// Target branch for delivery (from project settings if not specified)
    pub target_branch: Option<String>,
    /// Time limit override in minutes
    pub time_limit_minutes: Option<i64>,
}

/// Result of a successful dispatch
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchInfo {
    pub runtime_name: String,
    pub run_path: PathBuf,
    pub task_ids: Vec<String>,
    pub eval_ids: Vec<String>,
    pub spec_content: String,
    pub eval_content: Option<String>,
    pub target_branch: Option<String>,
    pub branch_off_commit: Option<String>,
}

/// Scope for a multi-root dispatch operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchScope {
    pub tasks: Vec<TaskTree>,
    pub evals: Vec<Eval>,
    pub task_ids: Vec<String>,
    pub eval_ids: Vec<String>,
    pub root_task_ids: Vec<String>,
}

/// Service for dispatching runs from board tasks
pub struct DispatchService {
    project_id: i64,
    board: BoardService,
}

impl DispatchService {
    /// Create a new dispatch service for a project
    pub fn new(project_id: i64) -> Self {
        Self {
            project_id,
            board: BoardService::new(project_id),
        }
    }

    /// Get a preview of what will be dispatched from a task
    pub fn preview_dispatch(&self, task_id: &str) -> DispatchResult<DispatchPreview> {
        Ok(block_on(self.board.preview_dispatch(task_id))?)
    }

    /// Get the full dispatch scope (tasks + evals) for a task
    pub fn get_dispatch_scope(&self, task_id: &str) -> DispatchResult<(Vec<TaskTree>, Vec<Eval>)> {
        let task_ids = block_on(self.board.get_subtree_task_ids(task_id))?;
        let evals = block_on(self.board.get_evals_for_tasks(&task_ids))?;

        // Get the task tree for the dispatch scope
        let all_tasks = block_on(self.board.get_tasks())?;
        let task_id_set: std::collections::HashSet<&String> = task_ids.iter().collect();
        let filtered_tasks: Vec<_> = all_tasks
            .into_iter()
            .filter(|t| task_id_set.contains(&t.id))
            .collect();
        let task_tree = self.board.build_task_tree(&filtered_tasks);

        Ok((task_tree, evals))
    }

    /// Get the full dispatch scope for multiple root tasks
    ///
    /// This collects all tasks from all root subtrees and their validating evals,
    /// deduplicating any overlapping tasks or evals.
    pub fn get_multi_dispatch_scope(
        &self,
        root_task_ids: &[String],
    ) -> DispatchResult<DispatchScope> {
        let mut all_task_ids: Vec<String> = Vec::new();
        let mut seen_tasks: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Collect all task IDs from all roots (deduplicated)
        for root_id in root_task_ids {
            let subtree_ids = block_on(self.board.get_subtree_task_ids(root_id))?;
            for tid in subtree_ids {
                if seen_tasks.insert(tid.clone()) {
                    all_task_ids.push(tid);
                }
            }
        }

        // Get evals that validate any of these tasks
        let evals = block_on(self.board.get_evals_for_tasks(&all_task_ids))?;
        let eval_ids: Vec<String> = evals.iter().map(|e| e.id.clone()).collect();

        // Build task tree from filtered tasks
        let all_tasks = block_on(self.board.get_tasks())?;
        let task_id_set: std::collections::HashSet<&String> = all_task_ids.iter().collect();
        let filtered_tasks: Vec<_> = all_tasks
            .into_iter()
            .filter(|t| task_id_set.contains(&t.id))
            .collect();
        let task_tree = self.board.build_task_tree(&filtered_tasks);

        Ok(DispatchScope {
            tasks: task_tree,
            evals,
            task_ids: all_task_ids,
            eval_ids,
            root_task_ids: root_task_ids.to_vec(),
        })
    }

    /// Generate a tasks.md file (read-only overview for humans)
    /// Unlike spec.md, this is just documentation - workers use MCP tools instead
    pub fn generate_tasks_md(&self, scope: &DispatchScope) -> String {
        let mut md = String::new();
        md.push_str("# Task Overview\n\n");
        md.push_str(
            "_This file is for human reference only. Workers access tasks via MCP tools._\n\n",
        );

        fn write_task(task: &TaskTree, depth: usize, md: &mut String) {
            let indent = "  ".repeat(depth);
            md.push_str(&format!("{}- **{}** (`{}`)\n", indent, task.name, task.id));

            if !task.content.is_empty() {
                // Include first line or first 100 chars as preview
                let preview: String = task
                    .content
                    .lines()
                    .next()
                    .map(|l| l.chars().take(100).collect())
                    .unwrap_or_default();
                if !preview.is_empty() {
                    md.push_str(&format!("{}  > {}\n", indent, preview));
                }
            }

            for child in &task.children {
                write_task(child, depth + 1, md);
            }
        }

        md.push_str("## Tasks\n\n");
        for task in &scope.tasks {
            write_task(task, 0, &mut md);
        }

        if !scope.evals.is_empty() {
            md.push_str("\n## Evals\n\n");
            for eval in &scope.evals {
                md.push_str(&format!("- **{}** (`{}`)\n", eval.name, eval.id));
                md.push_str(&format!("  Validates: {}\n", eval.validates.join(", ")));
            }
        }

        md
    }

    /// Create a board snapshot for the dispatch
    pub fn create_snapshot(&self, task_ids: &[String]) -> DispatchResult<BoardSnapshot> {
        Ok(block_on(self.board.create_dispatch_snapshot(task_ids))?)
    }

    /// Generate spec.md content from tasks
    pub fn generate_spec(&self, tasks: &[TaskTree]) -> String {
        let mut spec = String::new();
        spec.push_str("# Specification\n\n");

        fn write_task(task: &TaskTree, depth: usize, spec: &mut String) {
            let indent = "#".repeat(depth.min(5) + 1);
            spec.push_str(&format!("{} {}\n\n", indent, task.name));

            if !task.content.is_empty() {
                spec.push_str(&task.content);
                spec.push_str("\n\n");
            }

            for child in &task.children {
                write_task(child, depth + 1, spec);
            }
        }

        for task in tasks {
            write_task(task, 1, &mut spec);
        }

        spec
    }

    /// Generate eval.md content from evals
    pub fn generate_eval(&self, evals: &[Eval]) -> Option<String> {
        if evals.is_empty() {
            return None;
        }

        let mut content = String::new();
        content.push_str("# Evaluation Criteria\n\n");

        for eval in evals {
            content.push_str(&format!("## {}\n\n", eval.name));
            if !eval.content.is_empty() {
                content.push_str(&eval.content);
                content.push_str("\n\n");
            }
        }

        Some(content)
    }

    /// Record the dispatch in the task_runs table
    pub async fn record_dispatch(&self, task_id: &str, runtime_name: &str) -> DispatchResult<()> {
        self.board.record_task_run(task_id, runtime_name).await?;
        info!(
            "Recorded dispatch: task={}, run={}, project={}",
            task_id, runtime_name, self.project_id
        );
        Ok(())
    }

    /// Get all runs dispatched from a task
    pub async fn get_task_runs(
        &self,
        task_id: &str,
    ) -> DispatchResult<Vec<crate::core::board::TaskRun>> {
        Ok(self.board.get_runs_for_task(task_id).await?)
    }

    /// Get all task runs for the project
    pub async fn get_all_task_runs(&self) -> DispatchResult<Vec<crate::core::board::TaskRun>> {
        Ok(self.board.get_all_task_runs().await?)
    }

    /// Execute a full dispatch: create run, record in task_runs, return info
    ///
    /// Note: This method prepares the dispatch info but does NOT create the actual
    /// run directory/database. The caller (RunManager) is responsible for:
    /// 1. Creating the run with the generated spec/eval content
    /// 2. Setting up the git worktree/branch
    /// 3. Recording the board snapshot in the run's state
    pub fn prepare_dispatch(
        &self,
        task_id: &str,
        config: &DispatchConfig,
    ) -> DispatchResult<DispatchInfo> {
        // Get dispatch scope
        let (tasks, evals) = self.get_dispatch_scope(task_id)?;
        let preview = self.preview_dispatch(task_id)?;

        // Generate spec and eval content
        let spec_content = self.generate_spec(&tasks);
        let eval_content = self.generate_eval(&evals);

        // Generate run name if not provided
        let runtime_name = config
            .runtime_name
            .clone()
            .unwrap_or_else(crate::core::names::generate_runtime_name);

        info!(
            "Prepared dispatch: run={}, tasks={}, evals={}",
            runtime_name, preview.task_count, preview.eval_count
        );

        Ok(DispatchInfo {
            runtime_name,
            run_path: PathBuf::new(), // Will be set by RunManager
            task_ids: preview.task_ids,
            eval_ids: preview.eval_ids,
            spec_content,
            eval_content,
            target_branch: config.target_branch.clone(),
            branch_off_commit: None, // Will be set by RunManager
        })
    }

    /// Get the board service (for direct access if needed)
    pub fn board(&self) -> &BoardService {
        &self.board
    }

    /// Get project ID
    pub fn project_id(&self) -> i64 {
        self.project_id
    }

    /// Prepare a multi-root dispatch
    ///
    /// Like prepare_dispatch but accepts multiple root task IDs.
    /// Generates tasks.md instead of spec.md (workers use MCP tools).
    pub fn prepare_multi_dispatch(
        &self,
        root_task_ids: &[String],
        config: &DispatchConfig,
    ) -> DispatchResult<DispatchInfo> {
        let scope = self.get_multi_dispatch_scope(root_task_ids)?;

        // Generate tasks.md (for human reference)
        let spec_content = self.generate_tasks_md(&scope);

        // Generate eval content (still useful for reference)
        let eval_content = self.generate_eval(&scope.evals);

        // Generate run name if not provided
        let runtime_name = config
            .runtime_name
            .clone()
            .unwrap_or_else(crate::core::names::generate_runtime_name);

        info!(
            "Prepared multi-root dispatch: run={}, roots={}, tasks={}, evals={}",
            runtime_name,
            root_task_ids.len(),
            scope.task_ids.len(),
            scope.eval_ids.len()
        );

        Ok(DispatchInfo {
            runtime_name,
            run_path: PathBuf::new(), // Will be set by RunManager
            task_ids: scope.task_ids,
            eval_ids: scope.eval_ids,
            spec_content,
            eval_content,
            target_branch: config.target_branch.clone(),
            branch_off_commit: None, // Will be set by RunManager
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_spec() {
        let service = DispatchService::new(1);

        let tasks = vec![TaskTree {
            id: "build-api".to_string(),
            name: "Build API".to_string(),
            status: crate::core::board::TaskStatus::Todo,
            content: "Create REST endpoints".to_string(),
            children: vec![TaskTree {
                id: "user-endpoints".to_string(),
                name: "User Endpoints".to_string(),
                status: crate::core::board::TaskStatus::Todo,
                content: "CRUD for users".to_string(),
                children: vec![],
                x: None,
                y: None,
                validated: None,
            }],
            x: None,
            y: None,
            validated: None,
        }];

        let spec = service.generate_spec(&tasks);
        assert!(spec.contains("# Specification"));
        assert!(spec.contains("## Build API"));
        assert!(spec.contains("### User Endpoints"));
        assert!(spec.contains("Create REST endpoints"));
        assert!(spec.contains("CRUD for users"));
    }

    #[test]
    fn test_generate_eval() {
        let service = DispatchService::new(1);

        let evals = vec![crate::core::board::Eval {
            id: "api-test".to_string(),
            name: "API Test".to_string(),
            status: crate::core::board::EvalStatus::Blocked,
            content: "Test all endpoints return 200".to_string(),
            validates: vec!["build-api".to_string()],
            x: None,
            y: None,
            created_at: String::new(),
            updated_at: String::new(),
        }];

        let eval = service.generate_eval(&evals);
        assert!(eval.is_some());
        let eval = eval.unwrap();
        assert!(eval.contains("# Evaluation Criteria"));
        assert!(eval.contains("## API Test"));
        assert!(eval.contains("Test all endpoints return 200"));
    }

    #[test]
    fn test_generate_eval_empty() {
        let service = DispatchService::new(1);
        let eval = service.generate_eval(&[]);
        assert!(eval.is_none());
    }
}
