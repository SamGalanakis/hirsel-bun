//! Dispatch Service - creates runs from board task nodes
//!
//! This module handles dispatching runs from the SpecFlow board:
//! - Getting dispatch scope (task + descendants + validating evals)
//! - Creating board snapshots at dispatch time
//! - Generating spec.md and eval.md from board content
//! - Recording dispatch in task_runs junction table

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

use crate::core::board::{BoardService, BoardSnapshot, DispatchPreview, Eval, TaskTree};
use crate::core::state::{SQLiteState, StateError, TaskType};

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
pub struct DispatchConfig {
    /// Custom run name (auto-generated if not provided)
    pub run_name: Option<String>,
    /// Target branch for delivery (from project settings if not specified)
    pub target_branch: Option<String>,
    /// Worker scale override
    pub worker_scale: Option<String>,
    /// Time limit override in minutes
    pub time_limit_minutes: Option<i64>,
}

impl Default for DispatchConfig {
    fn default() -> Self {
        Self {
            run_name: None,
            target_branch: None,
            worker_scale: None,
            time_limit_minutes: None,
        }
    }
}

/// Result of a successful dispatch
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchInfo {
    pub run_name: String,
    pub run_path: PathBuf,
    pub task_ids: Vec<String>,
    pub eval_ids: Vec<String>,
    pub spec_content: String,
    pub eval_content: Option<String>,
    pub target_branch: Option<String>,
    pub branch_off_commit: Option<String>,
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
        Ok(self.board.preview_dispatch(task_id)?)
    }

    /// Get the full dispatch scope (tasks + evals) for a task
    pub fn get_dispatch_scope(&self, task_id: &str) -> DispatchResult<(Vec<TaskTree>, Vec<Eval>)> {
        let task_ids = self.board.get_subtree_task_ids(task_id)?;
        let evals = self.board.get_evals_for_tasks(&task_ids)?;

        // Get the task tree for the dispatch scope
        let all_tasks = self.board.get_tasks()?;
        let task_id_set: std::collections::HashSet<&String> = task_ids.iter().collect();
        let filtered_tasks: Vec<_> = all_tasks
            .into_iter()
            .filter(|t| task_id_set.contains(&t.id))
            .collect();
        let task_tree = self.board.build_task_tree(&filtered_tasks);

        Ok((task_tree, evals))
    }

    /// Create a board snapshot for the dispatch
    pub fn create_snapshot(&self, task_ids: &[String]) -> DispatchResult<BoardSnapshot> {
        Ok(self.board.create_dispatch_snapshot(task_ids)?)
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
    pub fn record_dispatch(&self, task_id: &str, run_name: &str) -> DispatchResult<()> {
        self.board.record_task_run(task_id, run_name)?;
        info!(
            "Recorded dispatch: task={}, run={}, project={}",
            task_id, run_name, self.project_id
        );
        Ok(())
    }

    /// Get all runs dispatched from a task
    pub fn get_task_runs(&self, task_id: &str) -> DispatchResult<Vec<crate::core::board::TaskRun>> {
        Ok(self.board.get_runs_for_task(task_id)?)
    }

    /// Get all task runs for the project
    pub fn get_all_task_runs(&self) -> DispatchResult<Vec<crate::core::board::TaskRun>> {
        Ok(self.board.get_all_task_runs()?)
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
        let run_name = config
            .run_name
            .clone()
            .unwrap_or_else(|| crate::core::names::generate_run_name());

        info!(
            "Prepared dispatch: run={}, tasks={}, evals={}",
            run_name, preview.task_count, preview.eval_count
        );

        Ok(DispatchInfo {
            run_name,
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

    /// Create tasks in run state from the dispatch scope
    ///
    /// This creates:
    /// 1. Work tasks from the task tree (with parent_id and blocked_by relationships)
    /// 2. Eval tasks from the evals (with validates relationship)
    ///
    /// Returns the number of tasks created.
    pub fn create_run_tasks(
        &self,
        state: &SQLiteState,
        task_id: &str,
    ) -> DispatchResult<(usize, usize)> {
        let task_ids = self.board.get_subtree_task_ids(task_id)?;
        let evals = self.board.get_evals_for_tasks(&task_ids)?;
        let all_tasks = self.board.get_tasks()?;

        // Filter to only dispatched tasks
        let task_id_set: std::collections::HashSet<&String> = task_ids.iter().collect();
        let dispatched_tasks: Vec<_> = all_tasks
            .iter()
            .filter(|t| task_id_set.contains(&t.id))
            .collect();

        // Create work tasks
        let mut work_count = 0;
        for task in &dispatched_tasks {
            // Get parent_id only if parent is in scope
            let parent_id = task.parent_id.as_ref().and_then(|pid| {
                if task_id_set.contains(pid) {
                    Some(pid.as_str())
                } else {
                    None
                }
            });

            // Note: Board tasks don't have blocking relationships. Run task blocking
            // is determined by eval validation relationships, not predefined blocks.
            state.add_task_with_type(
                &task.id,
                &task.name,
                parent_id,
                None, // No blocking - determined by eval validation
                TaskType::Work,
                None,           // Work tasks don't have validates
                Some(&task.id), // board_task_id
            )?;
            work_count += 1;
        }

        // Create eval tasks
        let mut eval_count = 0;
        for eval in &evals {
            // Get validates list - only include tasks that are in scope
            let validates: Vec<&str> = eval
                .validates
                .iter()
                .filter(|vid| task_id_set.contains(*vid))
                .map(|s| s.as_str())
                .collect();

            if validates.is_empty() {
                continue; // Skip eval if it has no validated tasks in scope
            }

            state.add_task_with_type(
                &eval.id,
                &eval.name,
                None, // Eval tasks don't have parent
                None, // Eval blocking is handled by validates relationship
                TaskType::Eval,
                Some(&validates),
                Some(&eval.id), // board_task_id
            )?;
            eval_count += 1;
        }

        info!(
            "Created {} work tasks and {} eval tasks for dispatch of '{}'",
            work_count, eval_count, task_id
        );

        Ok((work_count, eval_count))
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
