//! Task methods
//!
//! Methods for managing tasks: adding, claiming, completing, deleting.
//!
//! ## Task Types
//! - **Work tasks**: Implementation tasks that produce code changes
//! - **Eval tasks**: Tasks that validate work tasks
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

use super::types::{
    DeltaTaskInput, EvalResult, StateError, StateResult, Task, TaskSource, TaskStatus, TaskType,
};
use super::SQLiteState;

impl SQLiteState {
    // =========================================================================
    // Task Methods
    // =========================================================================

    /// Create a Task from a row, without blocked_by (must be fetched separately)
    fn task_from_row_without_blockers(row: &Row) -> rusqlite::Result<Task> {
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
            content: row.get("content")?,
            blocked_by: vec![], // Will be populated by caller
            task_type: row
                .get::<_, Option<String>>("task_type")?
                .map(|s| TaskType::from_str(&s))
                .unwrap_or(TaskType::Work),
            eval_result: row
                .get::<_, Option<String>>("eval_result")?
                .and_then(|s| EvalResult::from_str(&s)),
            eval_feedback: row.get("eval_feedback")?,
            board_task_id: row.get("board_task_id")?,
            assigned_to: row.get("assigned_to")?,
            completed_by: row.get("completed_by")?,
            source: row
                .get::<_, Option<String>>("source")?
                .and_then(|s| TaskSource::from_str(&s))
                .unwrap_or(TaskSource::Spec),
        })
    }

    /// Get blocker IDs for a task from the junction table
    fn get_blocker_ids(&self, task_id: &str) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .db
            .prepare_cached("SELECT blocker_id FROM task_blockers WHERE task_id = ?1")?;
        let blocker_ids = stmt
            .query_map(params![task_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(blocker_ids)
    }

    /// Get a task with its blockers populated
    pub(super) fn task_from_row(row: &Row) -> rusqlite::Result<Task> {
        Self::task_from_row_without_blockers(row)
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

    /// Add a blocker to a task
    ///
    /// This adds an entry to the task_blockers junction table indicating that
    /// task_id is blocked by blocker_id.
    pub fn add_blocker(&self, task_id: &str, blocker_id: &str) -> StateResult<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO task_blockers (task_id, blocker_id) VALUES (?1, ?2)",
            params![task_id, blocker_id],
        )?;
        self.log_history(
            "task_block",
            Some(&format!("{} blocked by {}", task_id, blocker_id)),
        )?;
        Ok(())
    }

    /// Set the assigned_to field on a task
    ///
    /// This is used by the coordinator to directly assign tasks to workers
    /// instead of workers claiming tasks themselves.
    pub fn set_task_assigned_to(
        &self,
        task_id: &str,
        worker_name: Option<&str>,
    ) -> StateResult<()> {
        let affected = self.db.execute(
            "UPDATE tasks SET assigned_to = ?1 WHERE id = ?2",
            params![worker_name, task_id],
        )?;
        if affected == 0 {
            return Err(StateError::NotFound(format!(
                "Task '{}' not found",
                task_id
            )));
        }
        if let Some(name) = worker_name {
            self.log_history("task_assign", Some(&format!("{} → {}", task_id, name)))?;
        }
        Ok(())
    }

    /// Get children of a task
    pub fn get_children(&self, task_id: &str) -> StateResult<Vec<Task>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, content, task_type, eval_result, eval_feedback, board_task_id, assigned_to, completed_by, source FROM tasks WHERE parent_id = ?1 ORDER BY created_at"
        )?;
        let mut tasks: Vec<Task> = stmt
            .query_map(params![task_id], Self::task_from_row)?
            .collect::<Result<Vec<_>, _>>()?;

        // Populate blockers for all tasks
        for task in &mut tasks {
            task.blocked_by = self.get_blocker_ids(&task.id)?;
        }
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

    /// Add a new task (defaults to Worker source since this is used by workers via MCP)
    pub fn add_task(
        &self,
        task_id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
    ) -> StateResult<()> {
        self.add_task_with_source(
            task_id,
            name,
            parent_id,
            blocked_by,
            TaskType::Work,
            None,
            None,
            None,
            TaskSource::Worker,
        )
    }

    /// Add a new task with explicit source (for system tasks like scope)
    pub fn add_task_with_source_simple(
        &self,
        task_id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
        source: TaskSource,
    ) -> StateResult<()> {
        self.add_task_with_source(
            task_id,
            name,
            parent_id,
            blocked_by,
            TaskType::Work,
            None,
            None,
            None,
            source,
        )
    }

    /// Add a new task with explicit type and validates (defaults to Spec source)
    pub fn add_task_with_type(
        &self,
        task_id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
        task_type: TaskType,
        validates: Option<&[&str]>,
        board_task_id: Option<&str>,
        content: Option<&str>,
    ) -> StateResult<()> {
        self.add_task_with_source(
            task_id,
            name,
            parent_id,
            blocked_by,
            task_type,
            validates,
            board_task_id,
            content,
            TaskSource::Spec,
        )
    }

    /// Add a new task with explicit type, validates, and source
    pub fn add_task_with_source(
        &self,
        task_id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
        task_type: TaskType,
        validates: Option<&[&str]>,
        board_task_id: Option<&str>,
        content: Option<&str>,
        source: TaskSource,
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

        match self.db.execute(
            "INSERT INTO tasks (id, name, status, created_at, parent_id, content, task_type, board_task_id, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![task_id, name, TaskStatus::Todo.as_str(), self.now(), parent_id, content, task_type.as_str(), board_task_id, source.as_str()],
        ) {
            Ok(_) => {
                // Insert task_blockers relationships
                if let Some(b) = blocked_by {
                    let mut stmt = self
                        .db
                        .prepare_cached("INSERT INTO task_blockers (task_id, blocker_id) VALUES (?1, ?2)")?;
                    for blocker_id in b {
                        stmt.execute(params![task_id, blocker_id])?;
                    }
                }

                // Insert eval_validates relationships for eval tasks (prepared statement for efficiency)
                if let Some(v) = validates {
                    let mut stmt = self
                        .db
                        .prepare_cached("INSERT INTO eval_validates (eval_id, task_id) VALUES (?1, ?2)")?;
                    for validated_task_id in v {
                        stmt.execute(params![task_id, validated_task_id])?;
                    }
                }

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

                // Trigger scaling check - new task may need a worker
                self.request_scaling_check()?;

                Ok(())
            }
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.extended_code == 1555 => {
                Err(StateError::AlreadyExists(format!("Task '{}' already exists", task_id)))
            }
            Err(e) => Err(StateError::Sqlite(e)),
        }
    }

    /// Add multiple tasks in a batch with deferred FK constraints
    ///
    /// This allows tasks to reference each other as blockers without requiring
    /// a specific insertion order. FK constraints are checked at commit time.
    pub fn add_tasks_batch(&self, tasks: &[DeltaTaskInput]) -> StateResult<()> {
        // Enable deferred FK checking for this connection
        self.db.execute("PRAGMA defer_foreign_keys = ON", [])?;

        // Use DEFERRED transaction (allows FK violations until commit)
        self.db.execute("BEGIN DEFERRED", [])?;

        let result = (|| -> StateResult<()> {
            for task in tasks {
                // Check hierarchy depth limit (max 3 levels)
                if let Some(ref pid) = task.parent_id {
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

                // Insert task (use INSERT OR IGNORE to skip duplicates gracefully)
                self.db.execute(
                "INSERT OR IGNORE INTO tasks (id, name, status, created_at, parent_id, content, task_type, board_task_id, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![&task.task_id, &task.name, TaskStatus::Todo.as_str(), self.now(), task.parent_id.as_deref(), task.content.as_deref(), task.task_type.as_str(), task.board_task_id.as_deref(), task.source.as_str()],
            )?;

                // Insert task_blockers relationships (ignore duplicates)
                if let Some(ref blockers) = task.blocked_by {
                    for blocker_id in blockers {
                        self.db.execute(
                        "INSERT OR IGNORE INTO task_blockers (task_id, blocker_id) VALUES (?1, ?2)",
                        params![&task.task_id, blocker_id],
                    )?;
                    }
                }

                // Insert eval_validates relationships for eval tasks (ignore duplicates)
                if let Some(ref validates) = task.validates {
                    for validated_task_id in validates {
                        self.db.execute(
                        "INSERT OR IGNORE INTO eval_validates (eval_id, task_id) VALUES (?1, ?2)",
                        params![&task.task_id, validated_task_id],
                    )?;
                    }
                }
            }

            Ok(())
        })();

        match result {
            Ok(()) => {
                // Commit - FK constraints are checked here
                self.db.execute("COMMIT", [])?;

                // Log history for each task
                for task in tasks {
                    let detail = format!(
                        "{}: {} ({})",
                        task.task_id,
                        task.name,
                        task.task_type.as_str()
                    );
                    let _ = self.log_history("task_add", Some(&detail));
                }

                // Trigger scaling check once after batch
                self.request_scaling_check()?;

                Ok(())
            }
            Err(e) => {
                let _ = self.db.execute("ROLLBACK", []);
                Err(e)
            }
        }
    }

    /// Get all tasks
    pub fn get_tasks(&self) -> StateResult<Vec<Task>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, content, task_type, eval_result, eval_feedback, board_task_id, assigned_to, completed_by, source FROM tasks ORDER BY created_at"
        )?;
        let mut tasks: Vec<Task> = stmt
            .query_map([], Self::task_from_row)?
            .collect::<Result<Vec<_>, _>>()?;

        // Populate blockers for all tasks
        for task in &mut tasks {
            task.blocked_by = self.get_blocker_ids(&task.id)?;
        }
        Ok(tasks)
    }

    /// Get a specific task
    pub fn get_task(&self, task_id: &str) -> StateResult<Option<Task>> {
        let result = self.db.query_row(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, content, task_type, eval_result, eval_feedback, board_task_id, assigned_to, completed_by, source FROM tasks WHERE id = ?1",
            params![task_id],
            Self::task_from_row,
        );
        match result {
            Ok(mut task) => {
                task.blocked_by = self.get_blocker_ids(&task.id)?;
                Ok(Some(task))
            }
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

        if task.blocked_by.is_empty() {
            return Ok(false);
        }

        for blocker_id in &task.blocked_by {
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
    ///
    /// A blocker is incomplete if:
    /// - It has a validating eval and is not yet `Validated`
    /// - It has no validating eval and is not yet `Done` or `Validated`
    pub fn get_blockers(&self, task_id: &str) -> StateResult<Vec<String>> {
        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Ok(vec![]),
        };

        if task.blocked_by.is_empty() {
            return Ok(vec![]);
        }

        let mut incomplete = vec![];
        for blocker_id in &task.blocked_by {
            if let Some(blocker) = self.get_task(blocker_id)? {
                let is_blocking = if self.has_validating_eval(blocker_id)? {
                    blocker.status != TaskStatus::Validated
                } else {
                    !blocker.status.is_complete()
                };
                if is_blocking {
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
    /// - Eval tasks are unblocked when all tasks in validates are done/awaiting_eval
    /// - Tasks with children cannot be claimed (work on leaf tasks instead)
    pub fn get_claimable_tasks(&self) -> StateResult<Vec<Task>> {
        let tasks = self.get_tasks()?;

        // Build maps for O(1) lookups
        let status_map: std::collections::HashMap<String, TaskStatus> =
            tasks.iter().map(|t| (t.id.clone(), t.status)).collect();

        // Build set of tasks that have children (parent_id references)
        let tasks_with_children: std::collections::HashSet<String> =
            tasks.iter().filter_map(|t| t.parent_id.clone()).collect();

        // Build set of tasks that have validating evals (from eval_validates table)
        let mut has_validating_eval: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut stmt = self
            .db
            .prepare("SELECT DISTINCT task_id FROM eval_validates")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let task_id: String = row.get(0)?;
            has_validating_eval.insert(task_id);
        }

        // Build map of eval_id -> validated task IDs
        let mut eval_validates_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut stmt = self
            .db
            .prepare("SELECT eval_id, task_id FROM eval_validates")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let eval_id: String = row.get(0)?;
            let task_id: String = row.get(1)?;
            eval_validates_map.entry(eval_id).or_default().push(task_id);
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
            // Skip tasks that have children - work on leaf tasks instead
            if tasks_with_children.contains(&task.id) {
                continue;
            }

            match task.task_type {
                TaskType::Eval => {
                    // Eval task: unblocked when all tasks in validates are done/awaiting_eval
                    let validates = eval_validates_map.get(&task.id);
                    let is_ready = validates
                        .map(|v| {
                            !v.is_empty()
                                && v.iter().all(|tid| {
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
                        })
                        .unwrap_or(false);

                    // Also check blocked_by (for repair flow)
                    let is_blocked = !task.blocked_by.is_empty()
                        && task.blocked_by.iter().any(|blocker_id| {
                            status_map
                                .get(blocker_id)
                                .map(|status| !status.is_complete())
                                .unwrap_or(false)
                        });

                    if is_ready && !is_blocked {
                        eval_tasks.push(task);
                    }
                }
                TaskType::Work => {
                    // Work task: blocked_by tasks must be Validated (or Done if no eval)
                    let is_blocked = !task.blocked_by.is_empty()
                        && task.blocked_by.iter().any(|blocker_id| {
                            let blocker_status = status_map.get(blocker_id);
                            let blocker_has_eval = has_validating_eval.contains(blocker_id);

                            match blocker_status {
                                Some(TaskStatus::Validated) => false, // Not blocked
                                Some(TaskStatus::Done) if !blocker_has_eval => false, // No eval required, done is enough
                                _ => true,                                            // Blocked
                            }
                        });

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

    /// Claim a task for a worker (legacy wrapper for backwards compatibility)
    pub fn claim_task(&self, task_id: &str, worker_name: &str) -> StateResult<()> {
        use super::types::ClaimTaskResult;
        match self.try_claim_task(task_id, worker_name)? {
            ClaimTaskResult::Success { .. } => Ok(()),
            ClaimTaskResult::Rejected { reason, .. } => {
                Err(StateError::InvalidState(format!("{:?}", reason)))
            }
        }
    }

    /// Try to claim a task, returning detailed rejection info on failure
    pub fn try_claim_task(
        &self,
        task_id: &str,
        worker_name: &str,
    ) -> StateResult<super::types::ClaimTaskResult> {
        use super::types::{ClaimRejectReason, ClaimTaskResult, TaskSummary};

        // Use BEGIN IMMEDIATE to prevent race conditions
        self.db.execute("BEGIN IMMEDIATE", [])?;

        let result = (|| -> StateResult<ClaimTaskResult> {
            // Helper to get claimable alternatives
            let get_alternatives = || -> Vec<TaskSummary> {
                self.get_claimable_tasks()
                    .unwrap_or_default()
                    .into_iter()
                    .take(5)
                    .map(|t| TaskSummary {
                        id: t.id,
                        name: t.name,
                        task_type: t.task_type,
                    })
                    .collect()
            };

            // 1. Task exists
            let task = match self.get_task(task_id)? {
                Some(t) => t,
                None => {
                    return Ok(ClaimTaskResult::Rejected {
                        reason: ClaimRejectReason::NotFound {
                            task_id: task_id.to_string(),
                        },
                        alternatives: get_alternatives(),
                    });
                }
            };

            // 2. Already done/validated
            if task.status != TaskStatus::Todo && task.status != TaskStatus::Doing {
                return Ok(ClaimTaskResult::Rejected {
                    reason: ClaimRejectReason::AlreadyComplete {
                        task_id: task_id.to_string(),
                        status: task.status,
                    },
                    alternatives: get_alternatives(),
                });
            }

            // 3. Claimed by other (not us)
            if task.status == TaskStatus::Doing {
                if task.claimed_by.as_deref() == Some(worker_name) {
                    // Already claimed by us - return success with current task
                    return Ok(ClaimTaskResult::Success { task });
                }
                return Ok(ClaimTaskResult::Rejected {
                    reason: ClaimRejectReason::ClaimedByOther {
                        task_id: task_id.to_string(),
                        claimed_by: task.claimed_by.unwrap_or_default(),
                    },
                    alternatives: get_alternatives(),
                });
            }

            // 4. Worker already has a task
            let existing: Option<String> = self
                .db
                .query_row(
                    "SELECT id FROM tasks WHERE claimed_by = ?1 AND status = ?2",
                    params![worker_name, TaskStatus::Doing.as_str()],
                    |row| row.get(0),
                )
                .ok();

            if let Some(existing_id) = existing {
                return Ok(ClaimTaskResult::Rejected {
                    reason: ClaimRejectReason::WorkerBusy {
                        existing_task_id: existing_id,
                    },
                    alternatives: vec![], // No alternatives when busy
                });
            }

            // 5. Has children
            if self.has_children(task_id)? {
                let children = self.get_children(task_id)?;
                return Ok(ClaimTaskResult::Rejected {
                    reason: ClaimRejectReason::HasChildren {
                        task_id: task_id.to_string(),
                        children: children.iter().map(|c| c.id.clone()).collect(),
                    },
                    alternatives: get_alternatives(),
                });
            }

            // 6 & 7. Check blocking based on task type
            match task.task_type {
                TaskType::Work => {
                    // Work tasks: blockers must be Validated (or Done if no eval)
                    let blockers = self.get_blocker_details(task_id)?;
                    if !blockers.is_empty() {
                        return Ok(ClaimTaskResult::Rejected {
                            reason: ClaimRejectReason::Blocked {
                                task_id: task_id.to_string(),
                                blockers,
                            },
                            alternatives: get_alternatives(),
                        });
                    }
                }
                TaskType::Eval => {
                    // Eval tasks: validates tasks must be Done/AwaitingEval/Validated
                    let pending = self.get_pending_validates(task_id)?;
                    if !pending.is_empty() {
                        return Ok(ClaimTaskResult::Rejected {
                            reason: ClaimRejectReason::EvalNotReady {
                                task_id: task_id.to_string(),
                                pending_tasks: pending,
                            },
                            alternatives: get_alternatives(),
                        });
                    }
                }
            }

            // All checks passed - claim the task
            self.db.execute(
                "UPDATE tasks SET status = ?1, claimed_by = ?2, claimed_at = ?3 WHERE id = ?4",
                params![TaskStatus::Doing.as_str(), worker_name, self.now(), task_id],
            )?;

            self.log_history(
                "task_claim",
                Some(&format!("{} by {}", task_id, worker_name)),
            )?;

            // Fetch the updated task
            let claimed_task = self.get_task(task_id)?.ok_or_else(|| {
                StateError::InvalidState("Task disappeared after claim".to_string())
            })?;

            Ok(ClaimTaskResult::Success { task: claimed_task })
        })();

        match result {
            Ok(claim_result) => {
                self.db.execute("COMMIT", [])?;
                Ok(claim_result)
            }
            Err(e) => {
                let _ = self.db.execute("ROLLBACK", []);
                Err(e)
            }
        }
    }

    /// Get detailed info about incomplete blockers for a task
    fn get_blocker_details(&self, task_id: &str) -> StateResult<Vec<super::types::BlockerInfo>> {
        use super::types::BlockerInfo;

        let task = match self.get_task(task_id)? {
            Some(t) => t,
            None => return Ok(vec![]),
        };

        if task.blocked_by.is_empty() {
            return Ok(vec![]);
        }

        let mut blockers = vec![];
        for blocker_id in &task.blocked_by {
            if let Some(blocker) = self.get_task(blocker_id)? {
                // Check if this blocker is actually blocking
                let is_blocking = if self.has_validating_eval(blocker_id)? {
                    blocker.status != TaskStatus::Validated
                } else {
                    !blocker.status.is_complete()
                };

                if is_blocking {
                    blockers.push(BlockerInfo {
                        task_id: blocker.id,
                        name: blocker.name,
                        status: blocker.status,
                        claimed_by: blocker.claimed_by,
                    });
                }
            }
        }
        Ok(blockers)
    }

    /// Get pending validates tasks for an eval (tasks not ready for eval)
    fn get_pending_validates(&self, eval_id: &str) -> StateResult<Vec<super::types::TaskSummary>> {
        use super::types::TaskSummary;

        let validates = self.get_validated_tasks(eval_id)?;
        let mut pending = vec![];

        for task_id in validates {
            if let Some(task) = self.get_task(&task_id)? {
                // Eval is ready when validates tasks are Done/AwaitingEval/Validated
                let is_ready = matches!(
                    task.status,
                    TaskStatus::Done | TaskStatus::AwaitingEval | TaskStatus::Validated
                );
                if !is_ready {
                    pending.push(TaskSummary {
                        id: task.id,
                        name: task.name,
                        task_type: task.task_type,
                    });
                }
            }
        }
        Ok(pending)
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

        // Trigger scaling check - unclaimed task may need a worker
        self.request_scaling_check()?;

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

        // Record completed_by for tree distance calculations
        self.db.execute(
            "UPDATE tasks SET completed_by = ?1 WHERE id = ?2",
            params![worker_name, task_id],
        )?;

        // Update worker's last_task_id (for tree distance) and clear assigned_task_id
        // This must happen so evaluate_scaling() knows this worker is ready for new work
        self.update_worker(
            worker_name,
            super::types::WorkerUpdate {
                last_task_id: Some(Some(task_id.to_string())),
                assigned_task_id: Some(None), // Clear assigned task
                ..Default::default()
            },
        )?;

        // Auto-complete parent if all siblings are done
        if let Some(parent_id) = &task.parent_id {
            self.maybe_complete_parent(parent_id)?;
        }

        // Trigger scaling check - completion may unblock other tasks
        self.request_scaling_check()?;

        Ok(())
    }

    /// Check if a task has a validating eval
    pub fn has_validating_eval(&self, task_id: &str) -> StateResult<bool> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM eval_validates WHERE task_id = ?1",
            params![task_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Get all task IDs validated by an eval
    pub fn get_validated_tasks(&self, eval_id: &str) -> StateResult<Vec<String>> {
        let mut stmt = self
            .db
            .prepare("SELECT task_id FROM eval_validates WHERE eval_id = ?1")?;
        let task_ids = stmt
            .query_map(params![eval_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(task_ids)
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

        // Get validated tasks before updating eval status
        let validated_task_ids = self.get_validated_tasks(eval_task_id)?;

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
        if !validated_task_ids.is_empty() {
            let placeholders = validated_task_ids
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "UPDATE tasks SET status = ?1 WHERE id IN ({}) AND status IN (?2, ?3)",
                placeholders
            );
            let mut params: Vec<Box<dyn rusqlite::ToSql>> =
                vec![Box::new(TaskStatus::Validated.as_str())];
            for tid in &validated_task_ids {
                params.push(Box::new(tid.clone()));
            }
            params.push(Box::new(TaskStatus::Done.as_str()));
            params.push(Box::new(TaskStatus::AwaitingEval.as_str()));
            self.db.execute(
                &sql,
                rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            )?;
        }

        self.log_history(
            "eval_pass",
            Some(&format!("{} by {}", eval_task_id, worker_name)),
        )?;

        // Update worker's last_task_id (for tree distance) and clear assigned_task_id
        self.update_worker(
            worker_name,
            super::types::WorkerUpdate {
                last_task_id: Some(Some(eval_task_id.to_string())),
                assigned_task_id: Some(None), // Clear assigned task
                ..Default::default()
            },
        )?;

        // Trigger scaling check - validation may unblock other tasks
        self.request_scaling_check()?;

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

        // Get validated tasks before creating repair
        let validated_task_ids = self.get_validated_tasks(eval_task_id)?;

        // Create repair task as child of eval
        let repair_id = format!(
            "{}-repair-{}",
            eval_task_id,
            self.now().replace([':', '-', '.'], "")
        );
        let repair_name = format!("Repair: {}", feedback.chars().take(50).collect::<String>());

        self.db.execute(
            "INSERT INTO tasks (id, name, status, created_at, parent_id, task_type, board_task_id, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                repair_id,
                repair_name,
                TaskStatus::Todo.as_str(),
                self.now(),
                eval_task_id,
                TaskType::Work.as_str(),
                task.board_task_id,
                TaskSource::Worker.as_str()
            ],
        )?;

        // Mark eval as pending (blocked by repair), set feedback
        self.db.execute(
            "UPDATE tasks SET status = ?1, eval_result = ?2, eval_feedback = ?3, claimed_by = NULL, claimed_at = NULL WHERE id = ?4",
            params![
                TaskStatus::Todo.as_str(),
                EvalResult::Fail.as_str(),
                feedback,
                eval_task_id
            ],
        )?;

        // Add blocking relationship (eval is now blocked by repair task)
        self.db.execute(
            "INSERT INTO task_blockers (task_id, blocker_id) VALUES (?1, ?2)",
            params![eval_task_id, repair_id],
        )?;

        // Mark validated tasks as needs_repair (batch update)
        if !validated_task_ids.is_empty() {
            let placeholders = validated_task_ids
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "UPDATE tasks SET status = ?1 WHERE id IN ({}) AND status IN (?2, ?3)",
                placeholders
            );
            let mut params: Vec<Box<dyn rusqlite::ToSql>> =
                vec![Box::new(TaskStatus::NeedsRepair.as_str())];
            for tid in &validated_task_ids {
                params.push(Box::new(tid.clone()));
            }
            params.push(Box::new(TaskStatus::Done.as_str()));
            params.push(Box::new(TaskStatus::AwaitingEval.as_str()));
            self.db.execute(
                &sql,
                rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            )?;
        }

        self.log_history(
            "eval_fail",
            Some(&format!(
                "{} by {} - {}",
                eval_task_id, worker_name, feedback
            )),
        )?;

        // Update worker's last_task_id (for tree distance) and clear assigned_task_id
        // Even though eval failed, the worker is done with this task
        self.update_worker(
            worker_name,
            super::types::WorkerUpdate {
                last_task_id: Some(Some(eval_task_id.to_string())),
                assigned_task_id: Some(None), // Clear assigned task
                ..Default::default()
            },
        )?;

        // Trigger scaling check - repair task created
        self.request_scaling_check()?;

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

        // Note: task_blockers rows are automatically cleaned up via CASCADE delete
        // when the blocker task is deleted

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

        // Trigger scaling check - deleting tasks may affect scaling needs
        self.request_scaling_check()?;

        Ok(())
    }

    /// Get claimed task for a worker
    pub fn get_claimed_task(&self, worker_name: &str) -> StateResult<Option<Task>> {
        let result = self.db.query_row(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, content, task_type, eval_result, eval_feedback, board_task_id, assigned_to, completed_by, source FROM tasks WHERE claimed_by = ?1 AND status = ?2",
            params![worker_name, TaskStatus::Doing.as_str()],
            Self::task_from_row,
        );
        match result {
            Ok(mut task) => {
                task.blocked_by = self.get_blocker_ids(&task.id)?;
                Ok(Some(task))
            }
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
