//! Draft node CRUD operations and tree building

use sqlx::Row;
use std::collections::HashMap;

use super::{DeltaState, DeltaStateError, DeltaStateResult};
use crate::core::db::utc_now;
use crate::core::delta::types::*;

impl DeltaState {
    // =========================================================================
    // Draft Node Operations
    // =========================================================================

    /// Get all draft nodes as flat list
    pub async fn get_draft_nodes(&self) -> DeltaStateResult<Vec<DraftNode>> {
        let pool = self.pool().await?;

        // Load relationships first
        let validates_map = self.load_draft_validates(pool).await?;
        let validated_by_map = self.load_draft_validated_by(pool).await?;
        let blocked_by_map = self.load_draft_blocked_by(pool).await?;

        let rows = sqlx::query(
            "SELECT id, project_id, parent_id, position, name, node_type, content, x, y, created_at, updated_at
             FROM draft_nodes
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
                DraftNode {
                    id: id.clone(),
                    project_id: row.get("project_id"),
                    parent_id: row.get("parent_id"),
                    position: row.get("position"),
                    name: row.get("name"),
                    node_type: NodeType::from_str(&row.get::<String, _>("node_type")),
                    content: row.get("content"),
                    validates: validates_map.get(&id).cloned().unwrap_or_default(),
                    validated_by: validated_by_map.get(&id).cloned().unwrap_or_default(),
                    blocked_by: blocked_by_map.get(&id).cloned().unwrap_or_default(),
                    x: row.get("x"),
                    y: row.get("y"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                }
            })
            .collect();

        Ok(nodes)
    }

    /// Get a single draft node
    pub async fn get_draft_node(&self, id: &str) -> DeltaStateResult<DraftNode> {
        let pool = self.pool().await?;

        let validates = self.load_draft_node_validates(pool, id).await?;
        let validated_by = self.load_draft_node_validated_by(pool, id).await?;
        let blocked_by = self.load_draft_node_blocked_by(pool, id).await?;

        let row = sqlx::query(
            "SELECT id, project_id, parent_id, position, name, node_type, content, x, y, created_at, updated_at
             FROM draft_nodes
             WHERE id = ? AND project_id = ? AND route_id = ?",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| DeltaStateError::DraftNodeNotFound(id.to_string()))?;

        Ok(DraftNode {
            id: row.get("id"),
            project_id: row.get("project_id"),
            parent_id: row.get("parent_id"),
            position: row.get("position"),
            name: row.get("name"),
            node_type: NodeType::from_str(&row.get::<String, _>("node_type")),
            content: row.get("content"),
            validates,
            validated_by,
            blocked_by,
            x: row.get("x"),
            y: row.get("y"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    /// Create a new draft node
    ///
    /// Validation rules:
    /// - If parent_id is provided, it must reference an existing node
    /// - If parent_id is not provided, auto-assign to root node (if one exists)
    pub async fn create_draft_node(
        &self,
        req: &CreateDraftNodeRequest,
    ) -> DeltaStateResult<DraftNode> {
        let pool = self.pool().await?;

        // Validate parent_id if provided
        if let Some(pid) = &req.parent_id {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM draft_nodes WHERE id = ? AND project_id = ? AND route_id = ?)",
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

        let id = self.generate_slug(pool, "draft_nodes", &req.name).await?;
        let now = utc_now();

        // Get position (append to end of siblings)
        let position: i32 = match &parent_id {
            Some(pid) => {
                let max: Option<i32> = sqlx::query_scalar(
                    "SELECT MAX(position) FROM draft_nodes WHERE parent_id = ? AND project_id = ? AND route_id = ? AND route_id = ?",
                )
                .bind(pid)
                .bind(self.project_id)
                .bind(self.route_id)
                .fetch_one(pool)
                .await?;
                max.unwrap_or(-1) + 1
            }
            None => {
                let max: Option<i32> = sqlx::query_scalar(
                    "SELECT MAX(position) FROM draft_nodes WHERE parent_id IS NULL AND project_id = ? AND route_id = ?",
                )
                .bind(self.project_id)
                .bind(self.route_id)
                .fetch_one(pool)
                .await?;
                max.unwrap_or(-1) + 1
            }
        };

        sqlx::query(
            "INSERT INTO draft_nodes (id, project_id, route_id, parent_id, position, name, node_type, content, x, y, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(&parent_id)
        .bind(position)
        .bind(&req.name)
        .bind(req.node_type.as_str())
        .bind(&req.content)
        .bind(req.x)
        .bind(req.y)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        // Insert validated_by relationships (task declares which evals validate it)
        for eval_id in &req.validated_by {
            sqlx::query(
                "INSERT INTO draft_node_validated_by (eval_id, task_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(eval_id)
            .bind(&id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        // Insert blocked_by relationships
        for blocker_id in &req.blocked_by {
            sqlx::query(
                "INSERT INTO draft_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(blocker_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        self.bump_tree_generation().await?;
        self.get_draft_node(&id).await
    }

    /// Update a draft node
    ///
    /// Special behavior: If updating a Project (root) node's name, the project
    /// name is also updated to keep them in sync.
    pub async fn update_draft_node(
        &self,
        id: &str,
        req: &UpdateDraftNodeRequest,
    ) -> DeltaStateResult<DraftNode> {
        let pool = self.pool().await?;

        // Verify exists
        let _current = self.get_draft_node(id).await?;

        let now = utc_now();

        // Build dynamic update query
        let mut sql = String::from("UPDATE draft_nodes SET updated_at = ?");
        let mut bind_index = 2; // 1 is now

        if req.name.is_some() {
            sql.push_str(&format!(", name = ?{}", bind_index));
            bind_index += 1;
        }
        if req.content.is_some() {
            sql.push_str(&format!(", content = ?{}", bind_index));
            bind_index += 1;
        }
        if req.x.is_some() {
            sql.push_str(&format!(", x = ?{}", bind_index));
            bind_index += 1;
        }
        if req.y.is_some() {
            sql.push_str(&format!(", y = ?{}", bind_index));
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
        if let Some(x) = req.x {
            query = query.bind(x);
        }
        if let Some(y) = req.y {
            query = query.bind(y);
        }
        query = query.bind(id).bind(self.project_id).bind(self.route_id);
        query.execute(pool).await?;

        // Replace validated_by if provided (task declares which evals validate it)
        if let Some(ref validated_by) = req.validated_by {
            sqlx::query(
                "DELETE FROM draft_node_validated_by WHERE task_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
            for eval_id in validated_by {
                sqlx::query(
                    "INSERT INTO draft_node_validated_by (eval_id, task_id, project_id, route_id) VALUES (?, ?, ?, ?)",
                )
                .bind(eval_id)
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
                "DELETE FROM draft_node_blocked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
            for blocker_id in blocked_by {
                sqlx::query(
                    "INSERT INTO draft_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
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
        self.get_draft_node(id).await
    }

    /// Delete a draft node and all its descendants
    pub async fn delete_draft_node(&self, id: &str) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        // Verify node exists
        let _node = self.get_draft_node(id).await?;

        // Collect all descendant IDs (recursive)
        let mut to_delete = vec![id.to_string()];
        let mut i = 0;
        while i < to_delete.len() {
            let parent_id = &to_delete[i];
            let children: Vec<String> = sqlx::query_scalar(
                "SELECT id FROM draft_nodes WHERE parent_id = ? AND project_id = ? AND route_id = ? AND route_id = ?",
            )
            .bind(parent_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .fetch_all(pool)
            .await?;
            to_delete.extend(children);
            i += 1;
        }

        // Delete all nodes and their relationships (children first due to potential FK constraints)
        for node_id in to_delete.iter().rev() {
            // Delete validated_by relationships (both as eval and as task)
            sqlx::query(
                "DELETE FROM draft_node_validated_by WHERE eval_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
            sqlx::query(
                "DELETE FROM draft_node_validated_by WHERE task_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
            // Delete blocked_by relationships (both as blocker and as blocked)
            sqlx::query(
                "DELETE FROM draft_node_blocked_by WHERE node_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
            sqlx::query(
                "DELETE FROM draft_node_blocked_by WHERE blocker_id = ? AND project_id = ? AND route_id = ?",
            )
            .bind(node_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
            // Delete the node itself
            sqlx::query("DELETE FROM draft_nodes WHERE id = ? AND project_id = ? AND route_id = ?")
                .bind(node_id)
                .bind(self.project_id)
                .bind(self.route_id)
                .execute(pool)
                .await?;
        }

        self.bump_tree_generation().await?;
        Ok(())
    }

    /// Reset tree - delete all draft nodes except the root (project node)
    ///
    /// This preserves the root node but removes all its children.
    pub async fn reset_tree(&self) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        // Delete all relationships (root node doesn't have validated_by/blocked_by)
        sqlx::query("DELETE FROM draft_node_validated_by WHERE project_id = ? AND route_id = ?")
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM draft_node_blocked_by WHERE project_id = ? AND route_id = ?")
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;

        // Delete all draft nodes except the root (where parent_id IS NOT NULL)
        sqlx::query(
            "DELETE FROM draft_nodes WHERE project_id = ? AND route_id = ? AND parent_id IS NOT NULL",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .execute(pool)
        .await?;

        self.bump_tree_generation().await?;
        Ok(())
    }

    /// Move a draft node to a new parent and/or position
    ///
    /// Validation:
    /// - Cannot move to null parent if a root already exists (would create second root)
    /// - If new_parent_id is provided, it must exist
    pub async fn move_draft_node(
        &self,
        id: &str,
        new_parent_id: Option<&str>,
        new_position: i32,
    ) -> DeltaStateResult<()> {
        let pool = self.pool().await?;

        // Verify node exists
        let _node = self.get_draft_node(id).await?;

        // Validate parent exists if specified
        if let Some(ref pid) = new_parent_id {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM draft_nodes WHERE id = ? AND project_id = ? AND route_id = ?)",
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
        // Moving to root (parent_id = None) is allowed - multiple roots are supported

        let now = utc_now();
        sqlx::query(
            "UPDATE draft_nodes SET parent_id = ?, position = ?, updated_at = ? WHERE id = ? AND project_id = ? AND route_id = ?",
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

    /// Create a draft node with a specific ID (for agent import)
    ///
    /// If the ID already exists, returns the existing node (idempotent).
    /// Otherwise creates a new node with the given ID.
    pub async fn create_draft_node_with_id(
        &self,
        id: &str,
        parent_id: Option<&str>,
        name: &str,
        node_type: NodeType,
        content: &str,
        validated_by: &[String],
        blocked_by: &[String],
    ) -> DeltaStateResult<DraftNode> {
        // Check if already exists
        match self.get_draft_node(id).await {
            Ok(existing) => return Ok(existing),
            Err(DeltaStateError::DraftNodeNotFound(_)) => {}
            Err(e) => return Err(e),
        }

        let pool = self.pool().await?;
        let now = utc_now();

        // Validate parent exists if specified
        if let Some(pid) = parent_id {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM draft_nodes WHERE id = ? AND project_id = ? AND route_id = ?)",
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

        // Get position (append to end of siblings)
        let position: i32 = {
            let max: Option<i32> = sqlx::query_scalar(
                "SELECT MAX(position) FROM draft_nodes WHERE parent_id IS ? AND project_id = ? AND route_id = ?",
            )
            .bind(parent_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .fetch_one(pool)
            .await?;
            max.unwrap_or(-1) + 1
        };

        sqlx::query(
            "INSERT INTO draft_nodes (id, project_id, route_id, parent_id, position, name, node_type, content, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(self.project_id)
        .bind(self.route_id)
        .bind(parent_id)
        .bind(position)
        .bind(name)
        .bind(node_type.as_str())
        .bind(content)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;

        // Insert validated_by relationships
        for eval_id in validated_by {
            sqlx::query(
                "INSERT INTO draft_node_validated_by (eval_id, task_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(eval_id)
            .bind(id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        // Insert blocked_by relationships
        for blocker_id in blocked_by {
            sqlx::query(
                "INSERT INTO draft_node_blocked_by (node_id, blocker_id, project_id, route_id) VALUES (?, ?, ?, ?)",
            )
            .bind(id)
            .bind(blocker_id)
            .bind(self.project_id)
            .bind(self.route_id)
            .execute(pool)
            .await?;
        }

        self.get_draft_node(id).await
    }

    /// Get the root node ID for this project
    pub async fn get_root_node_id(&self) -> DeltaStateResult<Option<String>> {
        let pool = self.pool().await?;
        let id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM draft_nodes WHERE parent_id IS NULL AND project_id = ? AND route_id = ? ORDER BY position LIMIT 1",
        )
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_optional(pool)
        .await?;
        Ok(id)
    }

    /// Get direct children of a node
    pub async fn get_children_of(&self, parent_id: &str) -> DeltaStateResult<Vec<DraftNode>> {
        let pool = self.pool().await?;

        // Load relationships
        let validates_map = self.load_draft_validates(pool).await?;
        let validated_by_map = self.load_draft_validated_by(pool).await?;
        let blocked_by_map = self.load_draft_blocked_by(pool).await?;

        let rows = sqlx::query(
            "SELECT id, project_id, parent_id, position, name, node_type, content, x, y, created_at, updated_at
             FROM draft_nodes
             WHERE parent_id = ? AND project_id = ? AND route_id = ?
             ORDER BY position",
        )
        .bind(parent_id)
        .bind(self.project_id)
        .bind(self.route_id)
        .fetch_all(pool)
        .await?;

        let nodes = rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                DraftNode {
                    id: id.clone(),
                    project_id: row.get("project_id"),
                    parent_id: row.get("parent_id"),
                    position: row.get("position"),
                    name: row.get("name"),
                    node_type: NodeType::from_str(&row.get::<String, _>("node_type")),
                    content: row.get("content"),
                    validates: validates_map.get(&id).cloned().unwrap_or_default(),
                    validated_by: validated_by_map.get(&id).cloned().unwrap_or_default(),
                    blocked_by: blocked_by_map.get(&id).cloned().unwrap_or_default(),
                    x: row.get("x"),
                    y: row.get("y"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                }
            })
            .collect();

        Ok(nodes)
    }

    /// Build tree from flat draft nodes
    ///
    /// Uses recursive approach to ensure all descendants are included.
    pub fn build_draft_tree(&self, nodes: &[DraftNode]) -> Vec<DraftNodeTree> {
        // Group children by parent
        let mut children_by_parent: HashMap<Option<String>, Vec<&DraftNode>> = HashMap::new();
        for node in nodes {
            children_by_parent
                .entry(node.parent_id.clone())
                .or_default()
                .push(node);
        }

        // Sort children by position
        for children in children_by_parent.values_mut() {
            children.sort_by_key(|n| n.position);
        }

        // Recursive tree builder
        fn build_node(
            node: &DraftNode,
            children_by_parent: &HashMap<Option<String>, Vec<&DraftNode>>,
        ) -> DraftNodeTree {
            let children: Vec<DraftNodeTree> = children_by_parent
                .get(&Some(node.id.clone()))
                .map(|kids| {
                    kids.iter()
                        .map(|child| build_node(child, children_by_parent))
                        .collect()
                })
                .unwrap_or_default();

            DraftNodeTree {
                id: node.id.clone(),
                name: node.name.clone(),
                node_type: node.node_type,
                content: node.content.clone(),
                validates: node.validates.clone(),
                validated_by: node.validated_by.clone(),
                blocked_by: node.blocked_by.clone(),
                children,
                x: node.x,
                y: node.y,
            }
        }

        // Build from roots (nodes with no parent)
        children_by_parent
            .get(&None)
            .map(|roots| {
                roots
                    .iter()
                    .map(|root| build_node(root, &children_by_parent))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get draft tree for project
    pub async fn get_draft_tree(&self) -> DeltaStateResult<Vec<DraftNodeTree>> {
        let nodes = self.get_draft_nodes().await?;
        Ok(self.build_draft_tree(&nodes))
    }
}
