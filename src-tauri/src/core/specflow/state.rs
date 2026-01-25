//! SpecFlow SQLite state management
//!
//! Board data is stored in the central hirsel.db with project_id foreign keys.
//! Deleting a project cascades to delete all its board data.

use rusqlite::{params, Connection, Row as SqliteRow};
use std::path::Path;
use uuid::Uuid;

use super::types::*;
use crate::core::config::global_db_path;

/// Schema for SpecFlow tables (in central DB)
const SCHEMA: &str = r#"
-- Islands are feature containers on the canvas
CREATE TABLE IF NOT EXISTS board_islands (
    id TEXT PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    x REAL NOT NULL DEFAULT 0,
    y REAL NOT NULL DEFAULT 0,
    width REAL DEFAULT 400,
    collapsed INTEGER DEFAULT 0,
    summary TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Rows within an island's Trifecta Grid
CREATE TABLE IF NOT EXISTS board_rows (
    id TEXT PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    island_id TEXT NOT NULL REFERENCES board_islands(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,

    -- Spec column (Intent)
    spec_content TEXT,
    spec_status TEXT DEFAULT 'draft',

    -- Task column (Reality)
    task_title TEXT,
    task_description TEXT,
    task_status TEXT DEFAULT 'todo',
    task_worker TEXT,
    task_blocked_by TEXT,  -- JSON array of row IDs

    -- Eval column (Proof)
    eval_criterion TEXT,
    eval_status TEXT DEFAULT 'pending',
    eval_result TEXT,

    -- Dispatch tracking
    dispatched INTEGER DEFAULT 0,
    run_name TEXT,

    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Dependency wires between islands
CREATE TABLE IF NOT EXISTS board_wires (
    id TEXT PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    from_island_id TEXT NOT NULL REFERENCES board_islands(id) ON DELETE CASCADE,
    to_island_id TEXT NOT NULL REFERENCES board_islands(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    UNIQUE(from_island_id, to_island_id)
);

-- Saved viewport positions (bookmarks)
CREATE TABLE IF NOT EXISTS board_bookmarks (
    id TEXT PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    x REAL NOT NULL,
    y REAL NOT NULL,
    zoom REAL NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_board_islands_project ON board_islands(project_id);
CREATE INDEX IF NOT EXISTS idx_board_rows_island ON board_rows(island_id);
CREATE INDEX IF NOT EXISTS idx_board_rows_project ON board_rows(project_id);
CREATE INDEX IF NOT EXISTS idx_board_wires_from ON board_wires(from_island_id);
CREATE INDEX IF NOT EXISTS idx_board_wires_to ON board_wires(to_island_id);
CREATE INDEX IF NOT EXISTS idx_board_wires_project ON board_wires(project_id);
CREATE INDEX IF NOT EXISTS idx_board_bookmarks_project ON board_bookmarks(project_id);
"#;

/// Error type for SpecFlow operations
#[derive(Debug, thiserror::Error)]
pub enum SpecFlowError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Island not found: {0}")]
    IslandNotFound(String),
    #[error("Row not found: {0}")]
    RowNotFound(String),
    #[error("Wire not found: {0}")]
    WireNotFound(String),
    #[error("Bookmark not found: {0}")]
    BookmarkNotFound(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type SpecFlowResult<T> = Result<T, SpecFlowError>;

/// SpecFlow state for a project (uses central DB)
pub struct SpecFlowState {
    db: Connection,
    project_id: i64,
}

impl SpecFlowState {
    /// Open or create the SpecFlow state for a project
    pub fn open(project_id: i64) -> SpecFlowResult<Self> {
        Self::open_at(&global_db_path(), project_id)
    }

    /// Open from a specific path (useful for testing)
    pub fn open_at(path: &Path, project_id: i64) -> SpecFlowResult<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "foreign_keys", "ON")?;

        let state = Self { db, project_id };
        state.init_db()?;
        Ok(state)
    }

    fn init_db(&self) -> SpecFlowResult<()> {
        self.db.execute_batch(SCHEMA)?;
        Ok(())
    }

    fn now(&self) -> String {
        chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string()
    }

    fn new_id(&self) -> String {
        Uuid::new_v4().to_string()
    }

    // ========== ISLAND OPERATIONS ==========

    /// Create a new island
    pub fn create_island(&self, req: &CreateIslandRequest) -> SpecFlowResult<Island> {
        let id = self.new_id();
        let now = self.now();

        self.db.execute(
            "INSERT INTO board_islands (id, project_id, name, x, y, width, collapsed, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)",
            params![&id, self.project_id, &req.name, req.x, req.y, req.width, &now, &now],
        )?;

        self.get_island(&id)
    }

    /// Get an island by ID (with rows)
    pub fn get_island(&self, id: &str) -> SpecFlowResult<Island> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, x, y, width, collapsed, summary, created_at, updated_at
             FROM board_islands WHERE id = ?1 AND project_id = ?2",
        )?;

        let island = stmt
            .query_row(params![id, self.project_id], |row| self.row_to_island(row))
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    SpecFlowError::IslandNotFound(id.to_string())
                }
                e => SpecFlowError::Database(e),
            })?;

        // Load rows for this island
        let rows = self.get_rows_for_island(id)?;

        Ok(Island { rows, ..island })
    }

    /// List all islands (with rows)
    pub fn list_islands(&self) -> SpecFlowResult<Vec<Island>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, x, y, width, collapsed, summary, created_at, updated_at
             FROM board_islands WHERE project_id = ?1 ORDER BY y, x",
        )?;

        let islands: Vec<Island> = stmt
            .query_map([self.project_id], |row| self.row_to_island(row))?
            .collect::<Result<Vec<_>, _>>()?;

        // Load rows for each island
        let mut result = Vec::with_capacity(islands.len());
        for island in islands {
            let rows = self.get_rows_for_island(&island.id)?;
            result.push(Island { rows, ..island });
        }

        Ok(result)
    }

    /// Update an island
    pub fn update_island(&self, id: &str, req: &UpdateIslandRequest) -> SpecFlowResult<Island> {
        // Check exists
        let _ = self.get_island(id)?;

        let now = self.now();

        // Build dynamic UPDATE
        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

        if let Some(ref name) = req.name {
            updates.push(format!("name = ?{}", params.len() + 1));
            params.push(Box::new(name.clone()));
        }
        if let Some(x) = req.x {
            updates.push(format!("x = ?{}", params.len() + 1));
            params.push(Box::new(x));
        }
        if let Some(y) = req.y {
            updates.push(format!("y = ?{}", params.len() + 1));
            params.push(Box::new(y));
        }
        if let Some(width) = req.width {
            updates.push(format!("width = ?{}", params.len() + 1));
            params.push(Box::new(width));
        }
        if let Some(collapsed) = req.collapsed {
            updates.push(format!("collapsed = ?{}", params.len() + 1));
            params.push(Box::new(collapsed as i64));
        }
        if let Some(ref summary) = req.summary {
            updates.push(format!("summary = ?{}", params.len() + 1));
            params.push(Box::new(summary.clone()));
        }

        params.push(Box::new(id.to_string()));
        params.push(Box::new(self.project_id));

        let sql = format!(
            "UPDATE board_islands SET {} WHERE id = ?{} AND project_id = ?{}",
            updates.join(", "),
            params.len() - 1,
            params.len()
        );

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        self.db.execute(&sql, param_refs.as_slice())?;

        self.get_island(id)
    }

    /// Delete an island (cascades to rows)
    pub fn delete_island(&self, id: &str) -> SpecFlowResult<()> {
        let deleted = self.db.execute(
            "DELETE FROM board_islands WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        if deleted == 0 {
            return Err(SpecFlowError::IslandNotFound(id.to_string()));
        }
        Ok(())
    }

    fn row_to_island(&self, row: &SqliteRow) -> rusqlite::Result<Island> {
        Ok(Island {
            id: row.get("id")?,
            name: row.get("name")?,
            x: row.get("x")?,
            y: row.get("y")?,
            width: row.get("width")?,
            collapsed: row.get::<_, i64>("collapsed")? != 0,
            summary: row.get("summary")?,
            rows: vec![], // Filled in by caller
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    // ========== ROW OPERATIONS ==========

    /// Get rows for an island
    fn get_rows_for_island(&self, island_id: &str) -> SpecFlowResult<Vec<Row>> {
        let mut stmt = self.db.prepare(
            "SELECT id, island_id, position,
                    spec_content, spec_status,
                    task_title, task_description, task_status, task_worker, task_blocked_by,
                    eval_criterion, eval_status, eval_result,
                    dispatched, run_name,
                    created_at, updated_at
             FROM board_rows
             WHERE island_id = ?1 AND project_id = ?2
             ORDER BY position",
        )?;

        let rows = stmt
            .query_map(params![island_id, self.project_id], |row| {
                self.sqlite_row_to_row(row)
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Create a new row
    pub fn create_row(&self, req: &CreateRowRequest) -> SpecFlowResult<Row> {
        // Verify island exists
        let _ = self.get_island(&req.island_id)?;

        let id = self.new_id();
        let now = self.now();

        // Get position (append to end if not specified)
        let position = match req.position {
            Some(pos) => pos,
            None => {
                let max_pos: i32 = self
                    .db
                    .query_row(
                        "SELECT COALESCE(MAX(position), -1) FROM board_rows WHERE island_id = ?1 AND project_id = ?2",
                        params![&req.island_id, self.project_id],
                        |row| row.get(0),
                    )
                    .unwrap_or(-1);
                max_pos + 1
            }
        };

        self.db.execute(
            "INSERT INTO board_rows (id, project_id, island_id, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![&id, self.project_id, &req.island_id, position, &now, &now],
        )?;

        self.get_row(&id)
    }

    /// Get a row by ID
    pub fn get_row(&self, id: &str) -> SpecFlowResult<Row> {
        let mut stmt = self.db.prepare(
            "SELECT id, island_id, position,
                    spec_content, spec_status,
                    task_title, task_description, task_status, task_worker, task_blocked_by,
                    eval_criterion, eval_status, eval_result,
                    dispatched, run_name,
                    created_at, updated_at
             FROM board_rows WHERE id = ?1 AND project_id = ?2",
        )?;

        stmt.query_row(params![id, self.project_id], |row| {
            self.sqlite_row_to_row(row)
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => SpecFlowError::RowNotFound(id.to_string()),
            e => SpecFlowError::Database(e),
        })
    }

    /// Update a row
    pub fn update_row(&self, id: &str, req: &UpdateRowRequest) -> SpecFlowResult<Row> {
        let _ = self.get_row(id)?;

        let now = self.now();

        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

        if let Some(ref v) = req.spec_content {
            updates.push(format!("spec_content = ?{}", params.len() + 1));
            params.push(Box::new(v.clone()));
        }
        if let Some(v) = req.spec_status {
            updates.push(format!("spec_status = ?{}", params.len() + 1));
            params.push(Box::new(v.as_str().to_string()));
        }
        if let Some(ref v) = req.task_title {
            updates.push(format!("task_title = ?{}", params.len() + 1));
            params.push(Box::new(v.clone()));
        }
        if let Some(ref v) = req.task_description {
            updates.push(format!("task_description = ?{}", params.len() + 1));
            params.push(Box::new(v.clone()));
        }
        if let Some(v) = req.task_status {
            updates.push(format!("task_status = ?{}", params.len() + 1));
            params.push(Box::new(v.as_str().to_string()));
        }
        if let Some(ref v) = req.task_worker {
            updates.push(format!("task_worker = ?{}", params.len() + 1));
            params.push(Box::new(v.clone()));
        }
        if let Some(ref v) = req.task_blocked_by {
            updates.push(format!("task_blocked_by = ?{}", params.len() + 1));
            params.push(Box::new(serde_json::to_string(v)?));
        }
        if let Some(ref v) = req.eval_criterion {
            updates.push(format!("eval_criterion = ?{}", params.len() + 1));
            params.push(Box::new(v.clone()));
        }
        if let Some(v) = req.eval_status {
            updates.push(format!("eval_status = ?{}", params.len() + 1));
            params.push(Box::new(v.as_str().to_string()));
        }
        if let Some(ref v) = req.eval_result {
            updates.push(format!("eval_result = ?{}", params.len() + 1));
            params.push(Box::new(v.clone()));
        }

        params.push(Box::new(id.to_string()));
        params.push(Box::new(self.project_id));

        let sql = format!(
            "UPDATE board_rows SET {} WHERE id = ?{} AND project_id = ?{}",
            updates.join(", "),
            params.len() - 1,
            params.len()
        );

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        self.db.execute(&sql, param_refs.as_slice())?;

        self.get_row(id)
    }

    /// Delete a row
    pub fn delete_row(&self, id: &str) -> SpecFlowResult<()> {
        let deleted = self.db.execute(
            "DELETE FROM board_rows WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        if deleted == 0 {
            return Err(SpecFlowError::RowNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Reorder rows within an island
    pub fn reorder_rows(&self, island_id: &str, row_ids: &[String]) -> SpecFlowResult<()> {
        let tx = self.db.unchecked_transaction()?;

        for (pos, row_id) in row_ids.iter().enumerate() {
            tx.execute(
                "UPDATE board_rows SET position = ?1, updated_at = ?2 WHERE id = ?3 AND island_id = ?4 AND project_id = ?5",
                params![pos as i32, self.now(), row_id, island_id, self.project_id],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Mark a row as dispatched
    pub fn set_row_dispatched(&self, row_id: &str, run_name: &str) -> SpecFlowResult<()> {
        self.db.execute(
            "UPDATE board_rows SET dispatched = 1, run_name = ?1, updated_at = ?2 WHERE id = ?3 AND project_id = ?4",
            params![run_name, self.now(), row_id, self.project_id],
        )?;
        Ok(())
    }

    /// Update row task status from run sync
    pub fn update_row_task_status(
        &self,
        row_id: &str,
        status: TaskStatus,
        worker: Option<String>,
    ) -> SpecFlowResult<()> {
        self.db.execute(
            "UPDATE board_rows SET task_status = ?1, task_worker = ?2, updated_at = ?3 WHERE id = ?4 AND project_id = ?5",
            params![status.as_str(), worker, self.now(), row_id, self.project_id],
        )?;
        Ok(())
    }

    /// Get rows with their blocked_by dependencies expanded
    pub fn get_rows_with_deps(&self, row_ids: &[String]) -> SpecFlowResult<Vec<Row>> {
        let mut result = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut to_visit: Vec<String> = row_ids.to_vec();

        while let Some(row_id) = to_visit.pop() {
            if visited.contains(&row_id) {
                continue;
            }
            visited.insert(row_id.clone());

            if let Ok(row) = self.get_row(&row_id) {
                // Add dependencies to visit list
                for dep_id in &row.task_blocked_by {
                    if !visited.contains(dep_id) {
                        to_visit.push(dep_id.clone());
                    }
                }
                result.push(row);
            }
        }

        Ok(result)
    }

    /// Check if any rows are already dispatched
    pub fn check_already_dispatched(&self, row_ids: &[String]) -> SpecFlowResult<Vec<String>> {
        let placeholders: Vec<String> = row_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let sql = format!(
            "SELECT id FROM board_rows WHERE id IN ({}) AND project_id = ?{} AND dispatched = 1",
            placeholders.join(", "),
            row_ids.len() + 1
        );

        let mut stmt = self.db.prepare(&sql)?;
        let mut params: Vec<&dyn rusqlite::ToSql> =
            row_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        params.push(&self.project_id);

        let ids: Vec<String> = stmt
            .query_map(params.as_slice(), |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ids)
    }

    fn sqlite_row_to_row(&self, row: &SqliteRow) -> rusqlite::Result<Row> {
        let blocked_by_json: Option<String> = row.get("task_blocked_by")?;
        let task_blocked_by: Vec<String> = blocked_by_json
            .map(|s| serde_json::from_str(&s).unwrap_or_default())
            .unwrap_or_default();

        Ok(Row {
            id: row.get("id")?,
            island_id: row.get("island_id")?,
            position: row.get("position")?,
            spec_content: row.get("spec_content")?,
            spec_status: SpecStatus::from_str(
                &row.get::<_, String>("spec_status").unwrap_or_default(),
            ),
            task_title: row.get("task_title")?,
            task_description: row.get("task_description")?,
            task_status: TaskStatus::from_str(
                &row.get::<_, String>("task_status").unwrap_or_default(),
            ),
            task_worker: row.get("task_worker")?,
            task_blocked_by,
            eval_criterion: row.get("eval_criterion")?,
            eval_status: RowEvalStatus::from_str(
                &row.get::<_, String>("eval_status").unwrap_or_default(),
            ),
            eval_result: row.get("eval_result")?,
            dispatched: row.get::<_, i64>("dispatched")? != 0,
            run_name: row.get("run_name")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    // ========== WIRE OPERATIONS ==========

    /// Create a wire between islands
    pub fn create_wire(&self, from_island_id: &str, to_island_id: &str) -> SpecFlowResult<Wire> {
        let id = self.new_id();
        let now = self.now();

        self.db.execute(
            "INSERT INTO board_wires (id, project_id, from_island_id, to_island_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![&id, self.project_id, from_island_id, to_island_id, &now],
        )?;

        self.get_wire(&id)
    }

    /// Get a wire by ID
    pub fn get_wire(&self, id: &str) -> SpecFlowResult<Wire> {
        let mut stmt = self.db.prepare(
            "SELECT id, from_island_id, to_island_id, created_at FROM board_wires WHERE id = ?1 AND project_id = ?2",
        )?;

        stmt.query_row(params![id, self.project_id], |row| {
            Ok(Wire {
                id: row.get("id")?,
                from_island_id: row.get("from_island_id")?,
                to_island_id: row.get("to_island_id")?,
                created_at: row.get("created_at")?,
            })
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => SpecFlowError::WireNotFound(id.to_string()),
            e => SpecFlowError::Database(e),
        })
    }

    /// List all wires
    pub fn list_wires(&self) -> SpecFlowResult<Vec<Wire>> {
        let mut stmt = self.db.prepare(
            "SELECT id, from_island_id, to_island_id, created_at FROM board_wires WHERE project_id = ?1",
        )?;

        let wires = stmt
            .query_map([self.project_id], |row| {
                Ok(Wire {
                    id: row.get("id")?,
                    from_island_id: row.get("from_island_id")?,
                    to_island_id: row.get("to_island_id")?,
                    created_at: row.get("created_at")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(wires)
    }

    /// Delete a wire
    pub fn delete_wire(&self, id: &str) -> SpecFlowResult<()> {
        let deleted = self.db.execute(
            "DELETE FROM board_wires WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        if deleted == 0 {
            return Err(SpecFlowError::WireNotFound(id.to_string()));
        }
        Ok(())
    }

    // ========== BOOKMARK OPERATIONS ==========

    /// Save a bookmark
    pub fn save_bookmark(&self, name: &str, x: f64, y: f64, zoom: f64) -> SpecFlowResult<Bookmark> {
        let id = self.new_id();
        let now = self.now();

        self.db.execute(
            "INSERT INTO board_bookmarks (id, project_id, name, x, y, zoom, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![&id, self.project_id, name, x, y, zoom, &now],
        )?;

        self.get_bookmark(&id)
    }

    /// Get a bookmark by ID
    pub fn get_bookmark(&self, id: &str) -> SpecFlowResult<Bookmark> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, x, y, zoom, created_at FROM board_bookmarks WHERE id = ?1 AND project_id = ?2",
        )?;

        stmt.query_row(params![id, self.project_id], |row| {
            Ok(Bookmark {
                id: row.get("id")?,
                name: row.get("name")?,
                x: row.get("x")?,
                y: row.get("y")?,
                zoom: row.get("zoom")?,
                created_at: row.get("created_at")?,
            })
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => SpecFlowError::BookmarkNotFound(id.to_string()),
            e => SpecFlowError::Database(e),
        })
    }

    /// List all bookmarks
    pub fn list_bookmarks(&self) -> SpecFlowResult<Vec<Bookmark>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, x, y, zoom, created_at FROM board_bookmarks WHERE project_id = ?1 ORDER BY created_at",
        )?;

        let bookmarks = stmt
            .query_map([self.project_id], |row| {
                Ok(Bookmark {
                    id: row.get("id")?,
                    name: row.get("name")?,
                    x: row.get("x")?,
                    y: row.get("y")?,
                    zoom: row.get("zoom")?,
                    created_at: row.get("created_at")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(bookmarks)
    }

    /// Delete a bookmark
    pub fn delete_bookmark(&self, id: &str) -> SpecFlowResult<()> {
        let deleted = self.db.execute(
            "DELETE FROM board_bookmarks WHERE id = ?1 AND project_id = ?2",
            params![id, self.project_id],
        )?;
        if deleted == 0 {
            return Err(SpecFlowError::BookmarkNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Get project ID
    pub fn project_id(&self) -> i64 {
        self.project_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_state() -> SpecFlowState {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("hirsel.db");

        // Create projects table first (simulating the real DB)
        let db = Connection::open(&db_path).unwrap();
        db.execute_batch(
            "CREATE TABLE projects (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             INSERT INTO projects (id, name) VALUES (1, 'Test Project');",
        )
        .unwrap();
        drop(db);

        SpecFlowState::open_at(&db_path, 1).unwrap()
    }

    #[test]
    fn test_island_crud() {
        let state = test_state();

        // Create
        let island = state
            .create_island(&CreateIslandRequest {
                name: "Auth".to_string(),
                x: 100.0,
                y: 200.0,
                width: 400.0,
            })
            .unwrap();
        assert_eq!(island.name, "Auth");
        assert_eq!(island.x, 100.0);

        // Update
        let updated = state
            .update_island(
                &island.id,
                &UpdateIslandRequest {
                    name: Some("Authentication".to_string()),
                    x: Some(150.0),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.name, "Authentication");
        assert_eq!(updated.x, 150.0);

        // List
        let islands = state.list_islands().unwrap();
        assert_eq!(islands.len(), 1);

        // Delete
        state.delete_island(&island.id).unwrap();
        assert!(state.get_island(&island.id).is_err());
    }

    #[test]
    fn test_row_crud() {
        let state = test_state();

        let island = state
            .create_island(&CreateIslandRequest {
                name: "Test".to_string(),
                x: 0.0,
                y: 0.0,
                width: 400.0,
            })
            .unwrap();

        // Create rows
        let row1 = state
            .create_row(&CreateRowRequest {
                island_id: island.id.clone(),
                position: None,
            })
            .unwrap();
        let row2 = state
            .create_row(&CreateRowRequest {
                island_id: island.id.clone(),
                position: None,
            })
            .unwrap();

        assert_eq!(row1.position, 0);
        assert_eq!(row2.position, 1);

        // Update row
        let updated = state
            .update_row(
                &row1.id,
                &UpdateRowRequest {
                    spec_content: Some("# Login Flow".to_string()),
                    task_title: Some("Implement login".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.spec_content, Some("# Login Flow".to_string()));
        assert_eq!(updated.task_title, Some("Implement login".to_string()));

        // Reorder
        state
            .reorder_rows(&island.id, &[row2.id.clone(), row1.id.clone()])
            .unwrap();
        let reordered = state.get_row(&row2.id).unwrap();
        assert_eq!(reordered.position, 0);

        // Delete
        state.delete_row(&row1.id).unwrap();
        let island = state.get_island(&island.id).unwrap();
        assert_eq!(island.rows.len(), 1);
    }

    #[test]
    fn test_wires() {
        let state = test_state();

        let island1 = state
            .create_island(&CreateIslandRequest {
                name: "A".to_string(),
                x: 0.0,
                y: 0.0,
                width: 400.0,
            })
            .unwrap();
        let island2 = state
            .create_island(&CreateIslandRequest {
                name: "B".to_string(),
                x: 500.0,
                y: 0.0,
                width: 400.0,
            })
            .unwrap();

        let wire = state.create_wire(&island1.id, &island2.id).unwrap();
        assert_eq!(wire.from_island_id, island1.id);
        assert_eq!(wire.to_island_id, island2.id);

        let wires = state.list_wires().unwrap();
        assert_eq!(wires.len(), 1);

        state.delete_wire(&wire.id).unwrap();
        assert!(state.list_wires().unwrap().is_empty());
    }

    #[test]
    fn test_bookmarks() {
        let state = test_state();

        let bookmark = state.save_bookmark("Overview", 0.0, 0.0, 1.0).unwrap();
        assert_eq!(bookmark.name, "Overview");

        let bookmarks = state.list_bookmarks().unwrap();
        assert_eq!(bookmarks.len(), 1);

        state.delete_bookmark(&bookmark.id).unwrap();
        assert!(state.list_bookmarks().unwrap().is_empty());
    }
}
