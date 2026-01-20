//! Task methods
//!
//! Methods for managing tasks: adding, claiming, completing, deleting.

use rusqlite::{params, Row};

use super::types::{StateError, StateResult, Task, TaskStatus};
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
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, blocked_by FROM tasks WHERE parent_id = ?1 ORDER BY created_at"
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

        match self.db.execute(
            "INSERT INTO tasks (id, name, status, created_at, parent_id, blocked_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![task_id, name, TaskStatus::Todo.as_str(), self.now(), parent_id, blocked_by_str],
        ) {
            Ok(_) => {
                let mut detail = format!("{}: {}", task_id, name);
                if let Some(pid) = parent_id {
                    detail.push_str(&format!(" (parent: {})", pid));
                }
                if let Some(b) = blocked_by {
                    detail.push_str(&format!(" (blocked by: {})", b.join(", ")));
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
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, blocked_by FROM tasks ORDER BY created_at"
        )?;
        let tasks = stmt
            .query_map([], Self::task_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    /// Get a specific task
    pub fn get_task(&self, task_id: &str) -> StateResult<Option<Task>> {
        let result = self.db.query_row(
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, blocked_by FROM tasks WHERE id = ?1",
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
                if blocker.status != TaskStatus::Done {
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
    /// Optimized to avoid N+1 queries by building a status lookup map
    pub fn get_claimable_tasks(&self) -> StateResult<Vec<Task>> {
        let tasks = self.get_tasks()?;

        // Build a map of task_id -> status for O(1) blocking checks
        let status_map: std::collections::HashMap<String, TaskStatus> =
            tasks.iter().map(|t| (t.id.clone(), t.status)).collect();

        let mut claimable = vec![];

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
            // Check blocking using the pre-built map instead of separate queries
            if let Some(blocked_by) = &task.blocked_by {
                if !blocked_by.is_empty() {
                    let is_blocked = blocked_by
                        .split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .any(|blocker_id| {
                            status_map
                                .get(blocker_id)
                                .map(|status| *status != TaskStatus::Done)
                                .unwrap_or(false)
                        });
                    if is_blocked {
                        continue;
                    }
                }
            }
            claimable.push(task);
        }
        Ok(claimable)
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

        self.db.execute(
            "UPDATE tasks SET status = ?1, completed_at = ?2 WHERE id = ?3",
            params![TaskStatus::Done.as_str(), self.now(), task_id],
        )?;

        self.log_history(
            "task_done",
            Some(&format!("{} by {}", task_id, worker_name)),
        )?;

        // Auto-complete parent if all siblings are done
        if let Some(parent_id) = &task.parent_id {
            self.maybe_complete_parent(parent_id)?;
        }

        Ok(())
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
            "SELECT id, name, status, created_at, completed_at, claimed_by, claimed_at, pending_done_at, tokens_used, parent_id, blocked_by FROM tasks WHERE claimed_by = ?1 AND status = ?2",
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
