//! Board node CRUD operations and tree building

use sqlx::Row;
use std::collections::HashMap;

use super::{DeltaState, DeltaStateError, DeltaStateResult};
use crate::core::db::utc_now;
use crate::core::delta::types::*;
use sqlx::QueryBuilder;

impl DeltaState {
    // =========================================================================
    // Board Node Read Operations
    // =========================================================================

    /// Get all board nodes as flat list
    pub async fn get_nodes(&self) -> DeltaStateResult<Vec<BoardNode>> {
        let pool = self.pool().await?;

        let validates_map = self.load_validates(pool).await?;
        let validated_by_map = self.load_validated_by(pool).await?;
        let blocked_by_map = self.load_blocked_by(pool).await?;

        let rows = sqlx::query(
            "SELECT id, project_id, parent_id, position, name, kind, source, content, difficulty, status, created_at, updated_at, completed_at, last_commit_sha, resolves, claimed_by, claimed_at, completed_by, check_result, check_feedback, tokens_used, assigned_agent_kind, assigned_agent_id, capability_profile, archived_at
             FROM board_nodes
             WHERE project_id = ? AND route_id = ?
             ORDER BY parent_id NULLS FIRST, position",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        let nodes = rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                self.row_to_node_with_relations(
                    &row,
                    &id,
                    &validates_map,
                    &validated_by_map,
                    &blocked_by_map,
                )
            })
            .collect();

        Ok(nodes)
    }

    /// Get a single board node
    pub async fn get_node(&self, id: &str) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;

        let validates = self.load_node_validates(pool, id).await?;
        let validated_by = self.load_node_validated_by(pool, id).await?;
        let blocked_by = self.load_node_blocked_by(pool, id).await?;

        let row = sqlx::query(
            "SELECT id, project_id, parent_id, position, name, kind, source, content, difficulty, status, created_at, updated_at, completed_at, last_commit_sha, resolves, claimed_by, claimed_at, completed_by, check_result, check_feedback, tokens_used, assigned_agent_kind, assigned_agent_id, capability_profile, archived_at
             FROM board_nodes
             WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DeltaStateError::NodeNotFound(id.to_string()))?;

        Ok(BoardNode {
            id: row.get("id"),
            project_id: row.get("project_id"),
            parent_id: row.get("parent_id"),
            position: row.get("position"),
            name: row.get("name"),
            kind: NodeKind::from_str(&row.get::<String, _>("kind")),
            source: BoardNodeSource::from_str(&row.get::<String, _>("source")),
            content: row.get("content"),
            difficulty: BoardNodeDifficulty::from_str(&row.get::<String, _>("difficulty")),
            status: BoardNodeStatus::from_str(&row.get::<String, _>("status")),
            validates,
            validated_by,
            blocked_by,
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            completed_at: row.get("completed_at"),
            last_commit_sha: row.get("last_commit_sha"),
            resolves: row.get("resolves"),
            claimed_by: row.get("claimed_by"),
            claimed_at: row.get("claimed_at"),
            completed_by: row.get("completed_by"),
            check_result: row
                .get::<Option<String>, _>("check_result")
                .and_then(|s| CheckResult::from_str(&s)),
            check_feedback: row.get("check_feedback"),
            tokens_used: row.get("tokens_used"),
            assigned_agent_kind: row.get("assigned_agent_kind"),
            assigned_agent_id: row.get("assigned_agent_id"),
            capability_profile: row
                .get::<Option<String>, _>("capability_profile")
                .and_then(|value| crate::core::CapabilityProfile::from_str(&value)),
            archived_at: row.get("archived_at"),
        })
    }

    /// Get all draft nodes (status=draft) for dispatch
    pub async fn get_draft_nodes(&self) -> DeltaStateResult<Vec<BoardNode>> {
        let nodes = self.get_nodes().await?;
        Ok(nodes
            .into_iter()
            .filter(|n| n.status == BoardNodeStatus::Draft)
            .collect())
    }

    /// Get the root node ID for this project
    pub async fn get_root_node_id(&self) -> DeltaStateResult<Option<String>> {
        let pool = self.pool().await?;
        let id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM board_nodes WHERE parent_id IS NULL AND project_id = ? AND route_id = ? ORDER BY position LIMIT 1",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?;
        Ok(id)
    }

    /// Get direct children of a node
    pub async fn get_children(&self, id: &str) -> DeltaStateResult<Vec<BoardNode>> {
        let pool = self.pool().await?;

        let validates_map = self.load_validates(pool).await?;
        let validated_by_map = self.load_validated_by(pool).await?;
        let blocked_by_map = self.load_blocked_by(pool).await?;

        let rows = sqlx::query(
            "SELECT id, project_id, parent_id, position, name, kind, source, content, difficulty, status, created_at, updated_at, completed_at, last_commit_sha, resolves, claimed_by, claimed_at, completed_by, check_result, check_feedback, tokens_used, assigned_agent_kind, assigned_agent_id, capability_profile, archived_at
             FROM board_nodes
             WHERE parent_id = ? AND project_id = ? AND route_id = ?
             ORDER BY position",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        let nodes = rows
            .into_iter()
            .map(|row| {
                let node_id: String = row.get("id");
                self.row_to_node_with_relations(
                    &row,
                    &node_id,
                    &validates_map,
                    &validated_by_map,
                    &blocked_by_map,
                )
            })
            .collect();

        Ok(nodes)
    }

    /// Check if a node has children
    pub async fn has_children(&self, id: &str) -> DeltaStateResult<bool> {
        let pool = self.pool().await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM board_nodes WHERE parent_id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_one(pool)
        .await?;
        Ok(count > 0)
    }

    /// Get a lightweight map of worker_name -> task_name for currently working nodes.
    pub async fn get_claimed_task_map(&self) -> DeltaStateResult<HashMap<String, String>> {
        let pool = self.pool().await?;

        let rows = sqlx::query(
            "SELECT claimed_by, name FROM board_nodes
             WHERE project_id = ? AND route_id = ? AND status = 'working' AND claimed_by IS NOT NULL",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let claimed_by: String = row.get("claimed_by");
                let name: String = row.get("name");
                (claimed_by, name)
            })
            .collect())
    }

    /// Get a lightweight map of node_id -> node_name for a set of IDs.
    ///
    /// This is used to display "current task" for workers that have an assigned_task_id
    /// but whose corresponding board node is not currently marked as working (or cannot
    /// be found in claimed_by queries).
    pub async fn get_node_name_map(
        &self,
        ids: &[String],
    ) -> DeltaStateResult<HashMap<String, String>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }

        let pool = self.pool().await?;
        let mut qb: QueryBuilder<sqlx::Sqlite> =
            QueryBuilder::new("SELECT id, name FROM board_nodes WHERE project_id = ");
        qb.push_bind(self.project_id);
        qb.push(" AND route_id = ");
        qb.push_bind(self.route_id);
        qb.push(" AND id IN (");

        let mut separated = qb.separated(", ");
        for id in ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");

        let rows = qb.build().fetch_all(pool).await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                let name: String = row.get("name");
                (id, name)
            })
            .collect())
    }

    /// Get summary counts for board nodes (done, total) without loading full data.
    /// Excludes draft nodes from counts.
    pub async fn get_node_counts(&self) -> DeltaStateResult<(u32, u32)> {
        let pool = self.pool().await?;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM board_nodes WHERE project_id = ? AND route_id = ? AND status != 'draft' AND archived_at IS NULL",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_one(pool)
        .await?;

        let done: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM board_nodes WHERE project_id = ? AND route_id = ? AND status IN ('done', 'validated') AND archived_at IS NULL",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_one(pool)
        .await?;

        Ok((done as u32, total as u32))
    }

    // =========================================================================
    // Board Node Create Operations
    // =========================================================================

    /// Create a new board node (feature/task/check from user)
    pub async fn create_node(&self, req: &CreateBoardNodeRequest) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;

        // Validate parent_id if provided
        if let Some(pid) = &req.parent_id {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM board_nodes WHERE id = ? AND project_id = ? AND route_id = ?)",
            )
            .bind(pid)
            .bind(self.project_id)
            .bind(self.route_id)
            .fetch_one(pool)
            .await?;
            if !exists {
                return Err(DeltaStateError::ParentNodeNotFound(pid.clone()));
            }
        }
        let parent_id = req.parent_id.clone();

        let id = self.generate_slug(pool, &req.name).await?;
        let now = utc_now();

        let position = self.next_position(pool, parent_id.as_deref()).await?;

        sqlx::query(
            "INSERT INTO board_nodes (id, project_id, route_id, parent_id, position, name, kind, source, content, difficulty, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'user', ?, ?, 'draft', ?, ?)",
        )
        .bind(&id)
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(&parent_id)
        .bind(position)
        .bind(&req.name)
        .bind(req.kind.as_str())
        .bind(&req.content)
        .bind(req.difficulty.as_str())
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        // Insert validated_by relationships
        for check_id in &req.validated_by {
            sqlx::query(
                "INSERT INTO board_node_checked_by (check_id, node_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(check_id)
            .bind(&id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        // Insert blocked_by relationships
        for blocker_id in &req.blocked_by {
            sqlx::query(
                "INSERT INTO board_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(blocker_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        self.bump_tree_generation().await?;
        self.get_node(&id).await
    }

    /// Create a node with a specific ID (for agent import or system tasks)
    ///
    /// If the ID already exists, returns the existing node (idempotent).
    pub async fn create_node_with_id(
        &self,
        id: &str,
        parent_id: Option<&str>,
        name: &str,
        kind: NodeKind,
        source: BoardNodeSource,
        content: &str,
        difficulty: BoardNodeDifficulty,
        status: BoardNodeStatus,
        validated_by: &[String],
        blocked_by: &[String],
    ) -> DeltaStateResult<BoardNode> {
        // Check if already exists
        match self.get_node(id).await {
            Ok(existing) => return Ok(existing),
            Err(DeltaStateError::NodeNotFound(_)) => {}
            Err(e) => return Err(e),
        }

        let pool = self.pool().await?;
        let now = utc_now();

        // Validate parent exists if specified
        if let Some(pid) = parent_id {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM board_nodes WHERE id = ? AND project_id = ? AND route_id = ?)",
            )
            .bind(pid)
            .bind(self.project_id)
            .bind(self.route_id)
            .fetch_one(pool)
            .await?;
            if !exists {
                return Err(DeltaStateError::ParentNodeNotFound(pid.to_string()));
            }
        }

        let position = self.next_position(pool, parent_id).await?;

        sqlx::query(
            "INSERT INTO board_nodes (id, project_id, route_id, parent_id, position, name, kind, source, content, difficulty, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(parent_id)
        .bind(position)
        .bind(name)
        .bind(kind.as_str())
        .bind(source.as_str())
        .bind(content)
        .bind(difficulty.as_str())
        .bind(status.as_str())
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        // Insert validated_by relationships
        for check_id in validated_by {
            sqlx::query(
                "INSERT INTO board_node_checked_by (check_id, node_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(check_id)
            .bind(id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        // Insert blocked_by relationships
        for blocker_id in blocked_by {
            sqlx::query(
                "INSERT INTO board_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(id)
            .bind(blocker_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        self.bump_tree_generation().await?;
        self.get_node(id).await
    }

    /// Create a node from a worker (source=worker, status=pending)
    pub async fn create_node_from_worker(
        &self,
        id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
        kind: NodeKind,
        content: &str,
        validates: Option<&[&str]>,
    ) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        let position = self.next_position(pool, parent_id).await?;

        sqlx::query(
            "INSERT INTO board_nodes (id, project_id, route_id, parent_id, position, name, kind, source, content, difficulty, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'worker', ?, ?, 'pending', ?, ?)",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(parent_id)
        .bind(position)
        .bind(name)
        .bind(kind.as_str())
        .bind(content)
        .bind(BoardNodeDifficulty::Medium.as_str())
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        if let Some(blockers) = blocked_by {
            for blocker_id in blockers {
                sqlx::query(
                    "INSERT OR IGNORE INTO board_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
                )
                .bind(id)
                .bind(blocker_id)
                .bind(self.project_id)
                .bind(self.route_id)
                .execute(pool)
                .await?;
            }
        }

        if let Some(target_ids) = validates {
            for target_id in target_ids {
                sqlx::query(
                    "INSERT OR IGNORE INTO board_node_checked_by (check_id, node_id, project_id, route_id) VALUES (?, ?, ?, ?)",
                )
                .bind(id)
                .bind(target_id)
                .bind(self.project_id)
                .bind(self.route_id)
                .execute(pool)
                .await?;
            }
        }

        self.bump_tree_generation().await?;
        self.get_node(id).await
    }

    // =========================================================================
    // Board Node Update Operations
    // =========================================================================

    /// Update a board node (for draft editing)
    pub async fn update_node(
        &self,
        id: &str,
        req: &UpdateBoardNodeRequest,
    ) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;

        let _current = self.get_node(id).await?;

        let now = utc_now();

        let mut sql = String::from("UPDATE board_nodes SET updated_at = ?");
        let mut bind_index = 2;

        if req.name.is_some() {
            sql.push_str(&format!(", name = ?{}", bind_index));
            bind_index += 1;
        }
        if req.content.is_some() {
            sql.push_str(&format!(", content = ?{}", bind_index));
            bind_index += 1;
        }
        if req.difficulty.is_some() {
            sql.push_str(&format!(", difficulty = ?{}", bind_index));
            bind_index += 1;
        }
        sql.push_str(&format!(
            " WHERE id = ?{} AND project_id = ?{} AND route_id = ?{}",
            bind_index,
            bind_index + 1,
            bind_index + 2
        ));

        let mut query = sqlx::query(&sql).bind(&now);
        if let Some(ref name) = req.name {
            query = query.bind(name);
        }
        if let Some(ref content) = req.content {
            query = query.bind(content);
        }
        if let Some(difficulty) = req.difficulty {
            query = query.bind(difficulty.as_str());
        }
        query = query.bind(id).bind(self.project_id).bind(self.route_id);
        query.execute(pool).await?;

        // Replace validated_by if provided
        if let Some(ref validated_by) = req.validated_by {
            sqlx::query(
                "DELETE FROM board_node_checked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
            for check_id in validated_by {
                sqlx::query(
                    "INSERT INTO board_node_checked_by (check_id, node_id, project_id, route_id) VALUES (?, ?, ?, ?)",
                )
                .bind(check_id)
                .bind(id)
                .bind(self.project_id)
                .bind(self.route_id)
                .execute(pool)
                .await?;
            }
        }

        // Replace blocked_by if provided
        if let Some(ref blocked_by) = req.blocked_by {
            sqlx::query(
                "DELETE FROM board_node_blocked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
            for blocker_id in blocked_by {
                sqlx::query(
                    "INSERT INTO board_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
                )
                .bind(id)
                .bind(blocker_id)
                .bind(self.project_id)
                .bind(self.route_id)
                .execute(pool)
                .await?;
            }
        }

        self.bump_tree_generation().await?;
        self.get_node(id).await
    }

    /// Update node status
    pub async fn update_node_status(
        &self,
        id: &str,
        status: BoardNodeStatus,
        commit_sha: Option<&str>,
    ) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;
        let now = utc_now();

        let completed_at = if matches!(
            status,
            BoardNodeStatus::Done | BoardNodeStatus::Failed | BoardNodeStatus::Validated
        ) {
            Some(now.clone())
        } else {
            None
        };

        sqlx::query(
            "UPDATE board_nodes SET status = ?, completed_at = ?, last_commit_sha = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(status.as_str())
        .bind(completed_at)
        .bind(commit_sha)
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        self.get_node(id).await
    }

    /// Create a new work item under the route root or a supplied parent.
    pub async fn create_work_item(
        &self,
        title: &str,
        parent_id: Option<&str>,
        description: &str,
        blocked_by: &[String],
    ) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;
        let now = utc_now();
        let target_parent = match parent_id {
            Some(id) => Some(id.to_string()),
            None => self.get_root_node_id().await?,
        };

        if let Some(ref pid) = target_parent {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM board_nodes WHERE id = ? AND project_id = ? AND route_id = ?)",
            )
            .bind(pid)
            .bind(self.project_id)
            .bind(self.route_id)
            .fetch_one(pool)
            .await?;
            if !exists {
                return Err(DeltaStateError::ParentNodeNotFound(pid.clone()));
            }
        }

        let id = self.generate_slug(pool, title).await?;
        let position = self.next_position(pool, target_parent.as_deref()).await?;
        sqlx::query(
            "INSERT INTO board_nodes (
                id, project_id, route_id, parent_id, position, name, kind, source,
                content, difficulty, status, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, 'task', 'user', ?, 'medium', 'pending', ?, ?)",
        )
        .bind(&id)
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(target_parent.as_deref())
        .bind(position)
        .bind(title)
        .bind(description)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        for blocker_id in blocked_by {
            sqlx::query(
                "INSERT INTO board_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(blocker_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        self.bump_tree_generation().await?;
        self.get_node(&id).await
    }

    /// Assign an item to a sub-orchestrator or worker.
    pub async fn assign_work_item(
        &self,
        id: &str,
        agent_kind: Option<&str>,
        agent_id: Option<&str>,
        capability_profile: Option<crate::core::CapabilityProfile>,
    ) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;
        let _ = self.get_node(id).await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE board_nodes
             SET assigned_agent_kind = ?, assigned_agent_id = ?, capability_profile = ?, updated_at = ?
             WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(agent_kind)
        .bind(agent_id)
        .bind(capability_profile.map(|profile| profile.as_str()))
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        self.get_node(id).await
    }

    /// Reopen an item so it can be worked again.
    pub async fn reopen_work_item(&self, id: &str) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;
        let _ = self.get_node(id).await?;
        let now = utc_now();

        sqlx::query(
            "UPDATE board_nodes
             SET status = 'pending',
                 claimed_by = NULL,
                 claimed_at = NULL,
                 completed_by = NULL,
                 completed_at = NULL,
                 check_result = NULL,
                 check_feedback = NULL,
                 capability_profile = NULL,
                 archived_at = NULL,
                 updated_at = ?
             WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        self.get_node(id).await
    }

    /// Archive an item and its descendants.
    pub async fn archive_work_item(&self, id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let node = self.get_node(id).await?;
        if node.parent_id.is_none() {
            return Err(DeltaStateError::NodeNotFound(
                "Cannot archive root item".to_string(),
            ));
        }

        let now = utc_now();
        let mut to_archive = vec![id.to_string()];
        let mut i = 0;
        while i < to_archive.len() {
            let parent_id = &to_archive[i];
            let children: Vec<String> = sqlx::query_scalar(
                "SELECT id FROM board_nodes WHERE parent_id = ? AND project_id = ? AND route_id = ? AND archived_at IS NULL",
            )
            .bind(parent_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .fetch_all(pool)
            .await?;
            to_archive.extend(children);
            i += 1;
        }

        for node_id in &to_archive {
            sqlx::query(
                "UPDATE board_nodes
                 SET archived_at = ?, assigned_agent_kind = NULL, assigned_agent_id = NULL, capability_profile = NULL, updated_at = ?
                 WHERE id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(&now)
            .bind(&now)
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        self.bump_tree_generation().await?;
        Ok(())
    }

    /// Mark an assigned work item as actively being worked.
    pub async fn start_work_item(&self, id: &str) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;
        let node = self.get_node(id).await?;
        if node.archived_at.is_some() {
            return Err(DeltaStateError::NodeNotFound(format!(
                "Node '{}' is archived and cannot be started",
                id
            )));
        }

        let now = utc_now();
        sqlx::query(
            "UPDATE board_nodes
             SET status = 'working',
                 claimed_by = NULL,
                 claimed_at = NULL,
                 completed_by = NULL,
                 completed_at = NULL,
                 check_result = NULL,
                 check_feedback = NULL,
                 updated_at = ?
             WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        self.get_node(id).await
    }

    /// Mark a work item as completed by a non-worker agent.
    pub async fn complete_work_item(
        &self,
        id: &str,
        completed_by: &str,
    ) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;
        let node = self.get_node(id).await?;
        if node.archived_at.is_some() {
            return Err(DeltaStateError::NodeNotFound(format!(
                "Node '{}' is archived and cannot be completed",
                id
            )));
        }

        let now = utc_now();
        sqlx::query(
            "UPDATE board_nodes
             SET status = 'done',
                 claimed_by = NULL,
                 claimed_at = NULL,
                 completed_by = ?,
                 completed_at = ?,
                 updated_at = ?
             WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(completed_by)
        .bind(&now)
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        self.get_node(id).await
    }

    /// Mark a work item as failed by a non-worker agent.
    pub async fn fail_work_item(
        &self,
        id: &str,
        completed_by: &str,
    ) -> DeltaStateResult<BoardNode> {
        let pool = self.pool().await?;
        let node = self.get_node(id).await?;
        if node.archived_at.is_some() {
            return Err(DeltaStateError::NodeNotFound(format!(
                "Node '{}' is archived and cannot be failed",
                id
            )));
        }

        let now = utc_now();
        sqlx::query(
            "UPDATE board_nodes
             SET status = 'failed',
                 claimed_by = NULL,
                 claimed_at = NULL,
                 completed_by = ?,
                 completed_at = ?,
                 updated_at = ?
             WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(completed_by)
        .bind(&now)
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        self.get_node(id).await
    }

    /// Record routine item activity without creating a separate escalation.
    pub async fn record_item_event(
        &self,
        item_id: &str,
        agent_kind: &str,
        agent_id: &str,
        event_kind: &str,
        summary: &str,
        details: Option<&str>,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;
        let _ = self.get_node(item_id).await?;
        let now = utc_now();

        sqlx::query(
            "INSERT INTO work_item_events (
                project_id, route_id, item_id, agent_kind, agent_id, event_kind, summary, details, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(item_id)
        .bind(agent_kind)
        .bind(agent_id)
        .bind(event_kind)
        .bind(summary)
        .bind(details)
        .bind(&now)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Move a node to a new parent and/or position
    pub async fn move_node(
        &self,
        id: &str,
        new_parent_id: Option<&str>,
        new_position: i32,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        let _node = self.get_node(id).await?;

        if let Some(ref pid) = new_parent_id {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM board_nodes WHERE id = ? AND project_id = ? AND route_id = ?)",
            )
            .bind(pid)
            .bind(self.project_id)
            .bind(self.route_id)
            .fetch_one(pool)
            .await?;
            if !exists {
                return Err(DeltaStateError::ParentNodeNotFound(pid.to_string()));
            }
        }

        let now = utc_now();
        sqlx::query(
            "UPDATE board_nodes SET parent_id = ?, position = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(new_parent_id)
        .bind(new_position)
        .bind(&now)
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        Ok(())
    }

    // =========================================================================
    // Board Node Delete Operations
    // =========================================================================

    /// Delete a node and all its descendants
    pub async fn delete_node(&self, id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        let node = self.get_node(id).await?;

        // Root nodes (no parent) cannot be deleted
        if node.parent_id.is_none() {
            return Err(DeltaStateError::NodeNotFound(
                "Cannot delete root node".to_string(),
            ));
        }

        // Collect all descendant IDs (recursive)
        let mut to_delete = vec![id.to_string()];
        let mut i = 0;
        while i < to_delete.len() {
            let parent_id = &to_delete[i];
            let children: Vec<String> = sqlx::query_scalar(
                "SELECT id FROM board_nodes WHERE parent_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(parent_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .fetch_all(pool)
            .await?;
            to_delete.extend(children);
            i += 1;
        }

        for node_id in to_delete.iter().rev() {
            sqlx::query(
                "DELETE FROM board_node_checked_by WHERE check_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
            sqlx::query(
                "DELETE FROM board_node_checked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
            sqlx::query(
                "DELETE FROM board_node_blocked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
            sqlx::query(
                "DELETE FROM board_node_blocked_by WHERE blocker_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
            sqlx::query("DELETE FROM board_nodes WHERE id = ? AND project_id = ? AND route_id = ?")
                .bind(node_id)
                .bind(self.project_id)
                .bind(self.route_id)
                .execute(pool)
                .await?;
        }

        self.bump_tree_generation().await?;
        Ok(())
    }

    /// Reset tree - delete all board nodes except the root
    pub async fn reset_tree(&self) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        sqlx::query("DELETE FROM board_node_checked_by WHERE project_id = ? AND route_id = ?")
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM board_node_blocked_by WHERE project_id = ? AND route_id = ?")
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;

        sqlx::query(
            "DELETE FROM board_nodes WHERE project_id = ? AND route_id = ? AND parent_id IS NOT NULL",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        Ok(())
    }

    // =========================================================================
    // Tree Building
    // =========================================================================

    /// Build tree from flat board nodes
    pub fn build_tree(&self, nodes: &[BoardNode]) -> Vec<BoardNodeTree> {
        let mut children_by_parent: HashMap<Option<String>, Vec<&BoardNode>> = HashMap::new();
        for node in nodes {
            children_by_parent
                .entry(node.parent_id.clone())
                .or_default()
                .push(node);
        }

        for children in children_by_parent.values_mut() {
            children.sort_by_key(|n| n.position);
        }

        fn build_node(
            node: &BoardNode,
            children_by_parent: &HashMap<Option<String>, Vec<&BoardNode>>,
        ) -> BoardNodeTree {
            let children: Vec<BoardNodeTree> = children_by_parent
                .get(&Some(node.id.clone()))
                .map(|kids| {
                    kids.iter()
                        .map(|child| build_node(child, children_by_parent))
                        .collect()
                })
                .unwrap_or_default();

            BoardNodeTree {
                id: node.id.clone(),
                parent_id: node.parent_id.clone(),
                name: node.name.clone(),
                kind: node.kind,
                source: node.source,
                content: node.content.clone(),
                difficulty: node.difficulty,
                status: node.status,
                validates: node.validates.clone(),
                validated_by: node.validated_by.clone(),
                blocked_by: node.blocked_by.clone(),
                completed_at: node.completed_at.clone(),
                last_commit_sha: node.last_commit_sha.clone(),
                resolves: node.resolves.clone(),
                children,
                claimed_by: node.claimed_by.clone(),
                claimed_at: node.claimed_at.clone(),
                completed_by: node.completed_by.clone(),
                check_result: node.check_result,
                check_feedback: node.check_feedback.clone(),
                tokens_used: node.tokens_used,
                assigned_agent_kind: node.assigned_agent_kind.clone(),
                assigned_agent_id: node.assigned_agent_id.clone(),
                capability_profile: node.capability_profile,
                archived_at: node.archived_at.clone(),
            }
        }

        let roots: Vec<&BoardNode> = children_by_parent.get(&None).cloned().unwrap_or_default();

        // If there are orphan root checks alongside a single root feature,
        // re-parent them under that feature so the graph stays connected.
        let root_features: Vec<&&BoardNode> = roots
            .iter()
            .filter(|n| n.kind == NodeKind::Feature || n.kind == NodeKind::Plan)
            .collect();

        if root_features.len() == 1 {
            let feature_id = root_features[0].id.clone();
            let orphan_checks: Vec<String> = roots
                .iter()
                .filter(|n| n.kind == NodeKind::Check)
                .map(|n| n.id.clone())
                .collect();

            if !orphan_checks.is_empty() {
                let feature_children = children_by_parent.entry(Some(feature_id)).or_default();
                for check_id in &orphan_checks {
                    if let Some(check_node) = nodes.iter().find(|n| n.id == *check_id) {
                        feature_children.push(check_node);
                    }
                }
                // Remove orphan checks from roots
                let filtered_roots: Vec<&BoardNode> = roots
                    .iter()
                    .filter(|n| !orphan_checks.contains(&n.id))
                    .copied()
                    .collect();
                return filtered_roots
                    .iter()
                    .map(|root| build_node(root, &children_by_parent))
                    .collect();
            }
        }

        roots
            .iter()
            .map(|root| build_node(root, &children_by_parent))
            .collect()
    }

    /// Get board tree for project
    pub async fn get_tree(&self) -> DeltaStateResult<Vec<BoardNodeTree>> {
        let nodes = self.get_nodes().await?;
        Ok(self.build_tree(&nodes))
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Get next position for a new child under parent
    async fn next_position(
        &self,
        pool: &sqlx::SqlitePool,
        parent_id: Option<&str>,
    ) -> DeltaStateResult<i32> {
        let max: Option<i32> = match parent_id {
            Some(pid) => {
                sqlx::query_scalar(
                    "SELECT MAX(position) FROM board_nodes WHERE parent_id = ? AND project_id = ? AND route_id = ?",
                )
                .bind(pid)
                .bind(self.project_id)
                .bind(self.route_id)
                .fetch_one(pool)
                .await?
            }
            None => {
                sqlx::query_scalar(
                    "SELECT MAX(position) FROM board_nodes WHERE parent_id IS NULL AND project_id = ? AND route_id = ?",
                )
                .bind(self.project_id)
                .bind(self.route_id)
                .fetch_one(pool)
                .await?
            }
        };
        Ok(max.unwrap_or(-1) + 1)
    }

    /// Convert a row to BoardNode using pre-loaded relation maps
    fn row_to_node_with_relations(
        &self,
        row: &sqlx::sqlite::SqliteRow,
        id: &str,
        validates_map: &HashMap<String, Vec<String>>,
        validated_by_map: &HashMap<String, Vec<String>>,
        blocked_by_map: &HashMap<String, Vec<String>>,
    ) -> BoardNode {
        BoardNode {
            id: id.to_string(),
            project_id: row.get("project_id"),
            parent_id: row.get("parent_id"),
            position: row.get("position"),
            name: row.get("name"),
            kind: NodeKind::from_str(&row.get::<String, _>("kind")),
            source: BoardNodeSource::from_str(&row.get::<String, _>("source")),
            content: row.get("content"),
            difficulty: BoardNodeDifficulty::from_str(&row.get::<String, _>("difficulty")),
            status: BoardNodeStatus::from_str(&row.get::<String, _>("status")),
            validates: validates_map.get(id).cloned().unwrap_or_default(),
            validated_by: validated_by_map.get(id).cloned().unwrap_or_default(),
            blocked_by: blocked_by_map.get(id).cloned().unwrap_or_default(),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            completed_at: row.get("completed_at"),
            last_commit_sha: row.get("last_commit_sha"),
            resolves: row.get("resolves"),
            claimed_by: row.get("claimed_by"),
            claimed_at: row.get("claimed_at"),
            completed_by: row.get("completed_by"),
            check_result: row
                .get::<Option<String>, _>("check_result")
                .and_then(|s| CheckResult::from_str(&s)),
            check_feedback: row.get("check_feedback"),
            tokens_used: row.get("tokens_used"),
            assigned_agent_kind: row.get("assigned_agent_kind"),
            assigned_agent_id: row.get("assigned_agent_id"),
            capability_profile: row
                .get::<Option<String>, _>("capability_profile")
                .and_then(|value| crate::core::CapabilityProfile::from_str(&value)),
            archived_at: row.get("archived_at"),
        }
    }
}
