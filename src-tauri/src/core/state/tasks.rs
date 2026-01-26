//! Task methods
//!
//! Methods for managing tasks: adding, claiming, completing, deleting.
//!
//! ## Task Types
//! - **Work tasks**: Implementation tasks that produce code changes
//! - **Eval tasks**: Validation tasks that verify work tasks
//!
//! ## Task Lifecycle
//! Work:  todo → doing → done → awaiting_eval → validated
//!                                ↓ (eval fails)
//!                           needs_repair → (repair done) → awaiting_eval
//!
//! Eval:  todo → doing → done (pass/fail)
//!                          ↓ (if failed)
//!                       blocked by repair task

use rusqlite::{params, Row};

use super::types::{EvalResult, StateError, StateResult, Task, TaskStatus, TaskType};
use super::SQLiteState;

impl SQLiteState {
    // =========================================================================
    // Task Methods
    // =========================================================================

    pub(super) fn task_from_row(row: &Row) -> rusqlite::Result<Task> {
        Ok(Task {
            id: row.get("id")?,
            name: row.get("name")?,
            status: TaskStatus::from_str(&row.get::<_, String>("status")?)
                .unwrap_or(TaskStatus::Todo),
            created_at: row.get("created_at")?,
            completed_at: row.get("completed_at")?,
            claimed_by: row.get("claimed_by")?,
            claimed_at: row.get("claimed_at")?,
            pending_done_at: row.get("pending_done_at")?,
            tokens_used: row.get("tokens_used")?,
            parent_id: row.get("parent_id")?,
            blocked_by: row.get("blocked_by")?,
            task_type: row
                .get::<_, Option<String>>("task_type")?
                .map(|s| TaskType::from_str(&s))
                .unwrap_or(TaskType::Work),
            validates: row.get("validates")?,
            eval_result: row
                .get::<_, Option<String>>("eval_result")?
                .and_then(|s| EvalResult::from_str(&s)),
            eval_feedback: row.get("eval_feedback")?,
            board_task_id: row.get("board_task_id")?,
        })
    }

    /// Get task depth in hierarchy
    pub fn get_task_depth(&self, task_id: &str) -> StateResult<usize> {
        let mut depth = 0;
        let mut current_id = Some(task_id.to_string());

        while let Some(id) = current_id {
            if let Some(task) = self.get_task(&id)? {
                depth += 1;
                current_id = task.parent_id;
            } else {
                break;
            }
        }
        Ok(depth)
    }

    /// Get children of a task
    pub fn get_children(&self, task_id: &str) -> StateResult<Vec<Task>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, blocked_by, task_type, validates, eval_result, eval_feedback, board_task_id FROM tasks WHERE parent_id = ?1 ORDER BY created_at"
        )?;
        let tasks = stmt
            .query_map(params![task_id], Self::task_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    /// Check if task has children
    pub fn has_children(&self, task_id: &str) -> StateResult<bool> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM tasks WHERE parent_id = ?1",
            params![task_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Add a new task
    pub fn add_task(
        &self,
        task_id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
    ) -> StateResult<()> {
        self.add_task_with_type(
            task_id,
            name,
            parent_id,
            blocked_by,
            TaskType::Work,
            None,
            None,
        )
    }

    /// Add a new task with explicit type and validates
    pub fn add_task_with_type(
        &self,
        task_id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
        task_type: TaskType,
        validates: Option<&[&str]>,
        board_task_id: Option<&str>,
    ) -> StateResult<()> {
        // Check hierarchy depth limit (max 3 levels)
        if let Some(pid) = parent_id {
            let parent_depth = self.get_task_depth(pid)?;
            if parent_depth == 0 {
                return Err(StateError::NotFound(format!(
                    "Parent task '{}' not found",
                    pid
                )));
            }
            if parent_depth >= 3 {
                return Err(StateError::InvalidState(format!(
                    "Cannot add child to '{}': max hierarchy depth is 3",
                    pid
                )));
            }
        }

        let blocked_by_str = blocked_by.map(|b| b.join(","));
        let validates_json = validates.map(|v| serde_json::to_string(&v).unwrap_or_default());

        match self.db.execute(
            "INSERT INTO tasks (id, name, status, created_at, parent_id, blocked_by, task_type, validates, board_task_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![task_id, name, TaskStatus::Todo.as_str(), self.now(), parent_id, blocked_by_str, task_type.as_str(), validates_json, board_task_id],
        ) {
            Ok(_) => {
                let mut detail = format!("{}: {} ({})", task_id, name, task_type.as_str());
                if let Some(pid) = parent_id {
                    detail.push_str(&format!(" (parent: {})", pid));
                }
                if let Some(b) = blocked_by {
                    detail.push_str(&format!(" (blocked by: {})", b.join(", ")));
                }
                if let Some(v) = validates {
                    detail.push_str(&format!(" (validates: {})", v.join(", ")));
                }
                self.log_history("task_add", Some(&detail))?;
                Ok(())
            }
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.extended_code == 1555 => {
                Err(StateError::AlreadyExists(format!("Task '{}' already exists", task_id)))
            }
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Get all tasks
    pub fn get_tasks(&self) -> StateResult<Vec<Task>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, blocked_by, task_type, validates, eval_result, eval_feedback, board_task_id FROM tasks ORDER BY created_at"
        )?;
        let tasks = stmt
            .query_map([], Self::task_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    /// Get a specific task
    pub fn get_task(&self, task_id: &str) -> StateResult<Option<Task>> {
        let result = self.db.query_row(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, blocked_by, task_type, validates, eval_result, eval_feedback, board_task_id FROM tasks WHERE id = ?1",
            params![task_id],
            Self::task_from_row,
        );
        match result {
            Ok(task) => Ok(Some(task)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Check if a task is blocked
    /// For work tasks: blockers must be Validated (or Done if no validating eval)
    /// For eval tasks: standard blocking on done status
    pub fn is_task_blocked(&self, task_id: &str) -> StateResult<bool> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Ok(false),
        };

        let blocked_by = match &task.blocked_by {
            Some(b) if !b.is_empty() => b,
            _ => return Ok(false),
        };

        for blocker_id in blocked_by
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            if let Some(blocker) = self.get_task(blocker_id)? {
                let is_blocking = match task.task_type {
                    TaskType::Work => {
                        // Work tasks require validation (or done if no eval)
                        if self.has_validating_eval(blocker_id)? {
                            blocker.status != TaskStatus::Validated
                        } else {
                            !blocker.status.is_complete()
                        }
                    }
                    TaskType::Eval => {
                        // Eval tasks just need completion
                        !blocker.status.is_complete()
                    }
                };
                if is_blocking {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Get incomplete blockers for a task
    pub fn get_blockers(&self, task_id: &str) -> StateResult<Vec<String>> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Ok(vec![]),
        };

        let blocked_by = match &task.blocked_by {
            Some(b) if !b.is_empty() => b,
            _ => return Ok(vec![]),
        };

        let mut incomplete = vec![];
        for blocker_id in blocked_by
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            if let Some(blocker) = self.get_task(blocker_id)? {
                if blocker.status != TaskStatus::Done {
                    incomplete.push(blocker_id.to_string());
                }
            }
        }
        Ok(incomplete)
    }

    /// Get tasks that can be claimed
    /// Optimized to avoid N+1 queries by building status/type lookup maps
    ///
    /// Priority order:
    /// 1. Eval tasks whose validated tasks are all done/awaiting_eval
    /// 2. Work tasks that are unblocked
    ///
    /// Blocking rules:
    /// - Work tasks blocked_by other tasks require those tasks to be Validated
    ///   (or Done if they have no validating eval)
    /// - Eval tasks are unblocked when all tasks in validates[] are done/awaiting_eval
    pub fn get_claimable_tasks(&self) -> StateResult<Vec<Task>> {
        let tasks = self.get_tasks()?;

        // Build maps for O(1) lookups
        let status_map: std::collections::HashMap<String, TaskStatus> =
            tasks.iter().map(|t| (t.id.clone(), t.status)).collect();
        let _type_map: std::collections::HashMap<String, TaskType> =
            tasks.iter().map(|t| (t.id.clone(), t.task_type)).collect();

        // Check if a task has a validating eval (any eval that references it)
        let mut has_validating_eval: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for task in &tasks {
            if task.task_type == TaskType::Eval {
                if let Some(validates_json) = &task.validates {
                    if let Ok(validates) = serde_json::from_str::<Vec<String>>(validates_json) {
                        for tid in validates {
                            has_validating_eval.insert(tid);
                        }
                    }
                }
            }
        }

        let mut eval_tasks = vec![];
        let mut work_tasks = vec![];

        for task in tasks {
            if task.status != TaskStatus::Todo {
                continue;
            }
            if task.claimed_by.is_some() {
                continue;
            }
            if task.id == "scope" {
                continue;
            }

            match task.task_type {
                TaskType::Eval => {
                    // Eval task: unblocked when all tasks in validates[] are done/awaiting_eval
                    let is_ready = if let Some(validates_json) = &task.validates {
                        if let Ok(validates) = serde_json::from_str::<Vec<String>>(validates_json) {
                            validates.iter().all(|tid| {
                                status_map
                                    .get(tid)
                                    .map(|s| {
                                        matches!(
                                            s,
                                            TaskStatus::Done
                                                | TaskStatus::AwaitingEval
                                                | TaskStatus::Validated
                                        )
                                    })
                                    .unwrap_or(false)
                            })
                        } else {
                            false
                        }
                    } else {
                        false // Eval with no validates is not ready
                    };

                    // Also check blocked_by (for repair flow)
                    let is_blocked = if let Some(blocked_by) = &task.blocked_by {
                        !blocked_by.is_empty()
                            && blocked_by
                                .split(',')
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .any(|blocker_id| {
                                    status_map
                                        .get(blocker_id)
                                        .map(|status| !status.is_complete())
                                        .unwrap_or(false)
                                })
                    } else {
                        false
                    };

                    if is_ready && !is_blocked {
                        eval_tasks.push(task);
                    }
                }
                TaskType::Work => {
                    // Work task: blocked_by tasks must be Validated (or Done if no eval)
                    let is_blocked = if let Some(blocked_by) = &task.blocked_by {
                        !blocked_by.is_empty()
                            && blocked_by
                                .split(',')
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .any(|blocker_id| {
                                    let blocker_status = status_map.get(blocker_id);
                                    let blocker_has_eval = has_validating_eval.contains(blocker_id);

                                    match blocker_status {
                                        Some(TaskStatus::Validated) => false, // Not blocked
                                        Some(TaskStatus::Done) if !blocker_has_eval => false, // No eval required, done is enough
                                        _ => true, // Blocked
                                    }
                                })
                    } else {
                        false
                    };

                    if !is_blocked {
                        work_tasks.push(task);
                    }
                }
            }
        }

        // Return eval tasks first (higher priority), then work tasks
        eval_tasks.extend(work_tasks);
        Ok(eval_tasks)
    }

    /// Claim a task for a worker
    pub fn claim_task(&self, task_id: &str, worker_name: &str) -> StateResult<()> {
        // Use BEGIN IMMEDIATE to prevent race conditions
        self.db.execute("BEGIN IMMEDIATE", [])?;

        let result = (|| -> StateResult<()> {
            let task = match self.get_task(task_id)? {
                Some(t) => t,
                None => {
                    return Err(StateError::NotFound(format!(
                        "Task '{}' not found",
                        task_id
                    )))
                }
            };

            if task.status != TaskStatus::Todo {
                return Err(StateError::InvalidState(format!(
                    "Task '{}' is not TODO (status: {})",
                    task_id, task.status
                )));
            }

            if self.has_children(task_id)? {
                return Err(StateError::InvalidState(format!(
                    "Task '{}' has children and cannot be claimed directly",
                    task_id
                )));
            }

            // Check if worker already has a claimed task
            let existing: Option<String> = self
                .db
                .query_row(
                    "SELECT id FROM tasks WHERE claimed_by = ?1 AND status = ?2",
                    params![worker_name, TaskStatus::Doing.as_str()],
                    |row| row.get(0),
                )
                .ok();

            if let Some(existing_id) = existing {
                return Err(StateError::InvalidState(format!(
                    "You already have task '{}' claimed. Complete or unclaim it first.",
                    existing_id
                )));
            }

            self.db.execute(
                "UPDATE tasks SET status = ?1, claimed_by = ?2, claimed_at = ?3 WHERE id = ?4",
                params![TaskStatus::Doing.as_str(), worker_name, self.now(), task_id],
            )?;

            self.log_history(
                "task_claim",
                Some(&format!("{} by {}", task_id, worker_name)),
            )?;
            Ok(())
        })();

        match result {
            Ok(_) => {
                self.db.execute("COMMIT", [])?;
                Ok(())
            }
            Err(e) => {
                let _ = self.db.execute("ROLLBACK", []);
                Err(e)
            }
        }
    }

    /// Unclaim a task
    pub fn unclaim_task(&self, task_id: &str, worker_name: &str) -> StateResult<()> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => {
                return Err(StateError::NotFound(format!(
                    "Task '{}' not found",
                    task_id
                )))
            }
        };

        if task.status != TaskStatus::Doing || task.claimed_by.as_deref() != Some(worker_name) {
            return Err(StateError::InvalidState(format!(
                "Task '{}' is not claimed by {}",
                task_id, worker_name
            )));
        }

        self.db.execute(
            "UPDATE tasks SET status = ?1, claimed_by = NULL, claimed_at = NULL WHERE id = ?2",
            params![TaskStatus::Todo.as_str(), task_id],
        )?;

        self.log_history(
            "task_unclaim",
            Some(&format!("{} by {}", task_id, worker_name)),
        )?;
        Ok(())
    }

    /// Complete a task
    /// For work tasks: sets status to done (or awaiting_eval if it has a validating eval)
    /// For eval tasks: use eval_pass or eval_fail instead
    pub fn complete_task(&self, task_id: &str, worker_name: &str) -> StateResult<()> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => {
                return Err(StateError::NotFound(format!(
                    "Task '{}' not found",
                    task_id
                )))
            }
        };

        if task.status != TaskStatus::Doing || task.claimed_by.as_deref() != Some(worker_name) {
            return Err(StateError::InvalidState(format!(
                "Task '{}' is not claimed by {}",
                task_id, worker_name
            )));
        }

        // For work tasks, check if there's a validating eval
        let new_status = if task.task_type == TaskType::Work {
            if self.has_validating_eval(task_id)? {
                TaskStatus::AwaitingEval
            } else {
                TaskStatus::Done
            }
        } else {
            // Eval tasks just go to done (eval_pass/eval_fail handle the result)
            TaskStatus::Done
        };

        self.db.execute(
            "UPDATE tasks SET status = ?1, completed_at = ?2 WHERE id = ?3",
            params![new_status.as_str(), self.now(), task_id],
        )?;

        self.log_history(
            "task_done",
            Some(&format!(
                "{} by {} ({})",
                task_id,
                worker_name,
                new_status.as_str()
            )),
        )?;

        // Auto-complete parent if all siblings are done
        if let Some(parent_id) = &task.parent_id {
            self.maybe_complete_parent(parent_id)?;
        }

        Ok(())
    }

    /// Check if a task has a validating eval
    pub fn has_validating_eval(&self, task_id: &str) -> StateResult<bool> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM tasks WHERE task_type = 'eval' AND validates LIKE ?1",
            params![format!("%\"{}%", task_id)],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Handle eval pass - validates all tasks in the validates list
    pub fn eval_pass(&self, eval_task_id: &str, worker_name: &str) -> StateResult<()> {
        let task = match self.get_task(eval_task_id)? {
            Some(t) => t,
            None => {
                return Err(StateError::NotFound(format!(
                    "Task '{}' not found",
                    eval_task_id
                )))
            }
        };

        if task.task_type != TaskType::Eval {
            return Err(StateError::InvalidState(format!(
                "Task '{}' is not an eval task",
                eval_task_id
            )));
        }

        if task.status != TaskStatus::Doing || task.claimed_by.as_deref() != Some(worker_name) {
            return Err(StateError::InvalidState(format!(
                "Eval task '{}' is not claimed by {}",
                eval_task_id, worker_name
            )));
        }

        // Mark eval as done with pass result
        self.db.execute(
            "UPDATE tasks SET status = ?1, completed_at = ?2, eval_result = ?3 WHERE id = ?4",
            params![
                TaskStatus::Done.as_str(),
                self.now(),
                EvalResult::Pass.as_str(),
                eval_task_id
            ],
        )?;

        // Validate all tasks in the validates list
        if let Some(validates_json) = &task.validates {
            if let Ok(validates) = serde_json::from_str::<Vec<String>>(validates_json) {
                for tid in validates {
                    self.db.execute(
                        "UPDATE tasks SET status = ?1 WHERE id = ?2 AND status IN (?3, ?4)",
                        params![
                            TaskStatus::Validated.as_str(),
                            tid,
                            TaskStatus::Done.as_str(),
                            TaskStatus::AwaitingEval.as_str()
                        ],
                    )?;
                }
            }
        }

        self.log_history(
            "eval_pass",
            Some(&format!("{} by {}", eval_task_id, worker_name)),
        )?;

        Ok(())
    }

    /// Handle eval fail - creates a repair task as child of the eval
    pub fn eval_fail(
        &self,
        eval_task_id: &str,
        worker_name: &str,
        feedback: &str,
    ) -> StateResult<String> {
        let task = match self.get_task(eval_task_id)? {
            Some(t) => t,
            None => {
                return Err(StateError::NotFound(format!(
                    "Task '{}' not found",
                    eval_task_id
                )))
            }
        };

        if task.task_type != TaskType::Eval {
            return Err(StateError::InvalidState(format!(
                "Task '{}' is not an eval task",
                eval_task_id
            )));
        }

        if task.status != TaskStatus::Doing || task.claimed_by.as_deref() != Some(worker_name) {
            return Err(StateError::InvalidState(format!(
                "Eval task '{}' is not claimed by {}",
                eval_task_id, worker_name
            )));
        }

        // Create repair task as child of eval
        let repair_id = format!(
            "{}-repair-{}",
            eval_task_id,
            self.now().replace([':', '-', '.'], "")
        );
        let repair_name = format!("Repair: {}", feedback.chars().take(50).collect::<String>());

        self.db.execute(
            "INSERT INTO tasks (id, name, status, created_at, parent_id, task_type, board_task_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                repair_id,
                repair_name,
                TaskStatus::Todo.as_str(),
                self.now(),
                eval_task_id,
                TaskType::Work.as_str(),
                task.board_task_id
            ],
        )?;

        // Mark eval as pending (blocked by repair), set feedback
        self.db.execute(
            "UPDATE tasks SET status = ?1, eval_result = ?2, eval_feedback = ?3, blocked_by = ?4, claimed_by = NULL, claimed_at = NULL WHERE id = ?5",
            params![
                TaskStatus::Todo.as_str(),
                EvalResult::Fail.as_str(),
                feedback,
                repair_id,
                eval_task_id
            ],
        )?;

        // Mark validated tasks as needs_repair
        if let Some(validates_json) = &task.validates {
            if let Ok(validates) = serde_json::from_str::<Vec<String>>(validates_json) {
                for tid in validates {
                    self.db.execute(
                        "UPDATE tasks SET status = ?1 WHERE id = ?2 AND status IN (?3, ?4)",
                        params![
                            TaskStatus::NeedsRepair.as_str(),
                            tid,
                            TaskStatus::Done.as_str(),
                            TaskStatus::AwaitingEval.as_str()
                        ],
                    )?;
                }
            }
        }

        self.log_history(
            "eval_fail",
            Some(&format!(
                "{} by {} - {}",
                eval_task_id, worker_name, feedback
            )),
        )?;

        Ok(repair_id)
    }

    fn maybe_complete_parent(&self, parent_id: &str) -> StateResult<()> {
        let children = self.get_children(parent_id)?;
        if children.is_empty() {
            return Ok(());
        }

        let all_done = children.iter().all(|c| c.status == TaskStatus::Done);
        if !all_done {
            return Ok(());
        }

        let parent = match self.get_task(parent_id)? {
            Some(p) => p,
            None => return Ok(()),
        };

        if parent.status == TaskStatus::Done {
            return Ok(());
        }

        self.db.execute(
            "UPDATE tasks SET status = ?1, completed_at = ?2 WHERE id = ?3",
            params![TaskStatus::Done.as_str(), self.now(), parent_id],
        )?;

        self.log_history(
            "task_done",
            Some(&format!("{} (auto-completed)", parent_id)),
        )?;

        // Recursively check grandparent
        if let Some(grandparent_id) = &parent.parent_id {
            self.maybe_complete_parent(grandparent_id)?;
        }

        Ok(())
    }

    /// Reopen a completed task
    pub fn reopen_task(&self, task_id: &str) -> StateResult<()> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => {
                return Err(StateError::NotFound(format!(
                    "Task '{}' not found",
                    task_id
                )))
            }
        };

        if task.status != TaskStatus::Done {
            return Err(StateError::InvalidState(format!(
                "Task '{}' is not done",
                task_id
            )));
        }

        self.db.execute(
            "UPDATE tasks SET status = ?1, claimed_by = NULL, claimed_at = NULL, completed_at = NULL WHERE id = ?2",
            params![TaskStatus::Todo.as_str(), task_id],
        )?;

        self.log_history("task_reopen", Some(task_id))?;
        Ok(())
    }

    /// Set pending_done_at for a task (first phase of two-phase completion)
    pub fn set_task_pending_done(&self, task_id: &str) -> StateResult<()> {
        self.db.execute(
            "UPDATE tasks SET pending_done_at = ?1 WHERE id = ?2",
            params![self.now(), task_id],
        )?;
        Ok(())
    }

    /// Clear pending_done_at for a task (second phase of two-phase completion)
    pub fn clear_task_pending_done(&self, task_id: &str) -> StateResult<()> {
        self.db.execute(
            "UPDATE tasks SET pending_done_at = NULL WHERE id = ?1",
            params![task_id],
        )?;
        Ok(())
    }

    /// Delete a task and all its children
    pub fn delete_task(&self, task_id: &str) -> StateResult<()> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => {
                return Err(StateError::NotFound(format!(
                    "Task '{}' not found",
                    task_id
                )))
            }
        };

        if task.status == TaskStatus::Doing {
            return Err(StateError::InvalidState(format!(
                "Task '{}' is currently claimed and cannot be deleted",
                task_id
            )));
        }

        // Collect all descendant IDs
        fn collect_descendants(state: &SQLiteState, tid: &str) -> StateResult<Vec<String>> {
            let children = state.get_children(tid)?;
            let mut descendants = vec![];
            for child in children {
                descendants.push(child.id.clone());
                descendants.extend(collect_descendants(state, &child.id)?);
            }
            Ok(descendants)
        }

        let mut all_to_delete = vec![task_id.to_string()];
        all_to_delete.extend(collect_descendants(self, task_id)?);

        // Check if any are claimed
        for tid in &all_to_delete {
            if let Some(t) = self.get_task(tid)? {
                if t.status == TaskStatus::Doing {
                    return Err(StateError::InvalidState(format!(
                        "Child task '{}' is currently claimed and cannot be deleted",
                        tid
                    )));
                }
            }
        }

        // Remove from blocked_by lists of other tasks
        let all_tasks = self.get_tasks()?;
        for t in all_tasks {
            if let Some(blocked_by) = &t.blocked_by {
                let blockers: Vec<&str> = blocked_by
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect();
                let new_blockers: Vec<&str> = blockers
                    .into_iter()
                    .filter(|b| !all_to_delete.contains(&b.to_string()))
                    .collect();
                if new_blockers.len() != blocked_by.split(',').count() {
                    let new_blocked_by = if new_blockers.is_empty() {
                        None
                    } else {
                        Some(new_blockers.join(","))
                    };
                    self.db.execute(
                        "UPDATE tasks SET blocked_by = ?1 WHERE id = ?2",
                        params![new_blocked_by, t.id],
                    )?;
                }
            }
        }

        // Delete all tasks
        for tid in &all_to_delete {
            self.db
                .execute("DELETE FROM tasks WHERE id = ?1", params![tid])?;
        }

        let detail = if all_to_delete.len() > 1 {
            format!("{} (and {} children)", task_id, all_to_delete.len() - 1)
        } else {
            task_id.to_string()
        };
        self.log_history("task_delete", Some(&detail))?;

        Ok(())
    }

    /// Get claimed task for a worker
    pub fn get_claimed_task(&self, worker_name: &str) -> StateResult<Option<Task>> {
        let result = self.db.query_row(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, blocked_by, task_type, validates, eval_result, eval_feedback, board_task_id FROM tasks WHERE claimed_by = ?1 AND status = ?2",
            params![worker_name, TaskStatus::Doing.as_str()],
            Self::task_from_row,
        );
        match result {
            Ok(task) => Ok(Some(task)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Set task tokens used
    pub fn set_task_tokens(&self, task_id: &str, tokens: i64) -> StateResult<()> {
        self.db.execute(
            "UPDATE tasks SET tokens_used = ?1 WHERE id = ?2",
            params![tokens, task_id],
        )?;
        Ok(())
    }

    /// Get average task tokens
    pub fn get_avg_task_tokens(&self) -> StateResult<i64> {
        match self.db.query_row(
            "SELECT AVG(tokens_used) FROM tasks WHERE status = 'done' AND tokens_used IS NOT NULL AND id != 'scope'",
            [],
            |row| row.get::<_, Option<f64>>(0),
        ) {
            Ok(Some(val)) => Ok(val as i64),
            Ok(None) => Ok(0),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Admin complete a task (bypasses claim check)
    pub fn admin_complete_task(&self, task_id: &str) -> StateResult<()> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => {
                return Err(StateError::NotFound(format!(
                    "Task '{}' not found",
                    task_id
                )))
            }
        };

        if task.status == TaskStatus::Done {
            return Ok(()); // Already done
        }

        self.db.execute(
            "UPDATE tasks SET status = ?1, completed_at = ?2 WHERE id = ?3",
            params![TaskStatus::Done.as_str(), self.now(), task_id],
        )?;

        self.log_history("task_done", Some(&format!("{} (admin)", task_id)))?;

        // Auto-complete parent if all siblings are done
        if let Some(parent_id) = &task.parent_id {
            self.maybe_complete_parent(parent_id)?;
        }

        Ok(())
    }

    /// Admin unclaim a task (bypasses worker check)
    pub fn admin_unclaim_task(&self, task_id: &str) -> StateResult<()> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => {
                return Err(StateError::NotFound(format!(
                    "Task '{}' not found",
                    task_id
                )))
            }
        };

        if task.status != TaskStatus::Doing {
            return Ok(()); // Not claimed
        }

        self.db.execute(
            "UPDATE tasks SET status = ?1, claimed_by = NULL, claimed_at = NULL WHERE id = ?2",
            params![TaskStatus::Todo.as_str(), task_id],
        )?;

        self.log_history("task_unclaim", Some(&format!("{} (admin)", task_id)))?;
        Ok(())
    }
}
