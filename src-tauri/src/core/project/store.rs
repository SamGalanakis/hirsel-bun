//! Project storage in global SQLite database

use rusqlite::{params, Connection, Row};
use std::path::Path;

use super::types::{CreateProjectRequest, Project, UpdateProjectRequest};
use crate::core::config::global_db_path;
use crate::core::draft::StartingPoint;

/// Schema for projects table
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    -- Normalized StartingPoint (no JSON blob)
    starting_point_type TEXT NOT NULL,  -- 'greenfield' | 'local_folder' | 'git_repo'
    starting_point_path TEXT,           -- for local_folder
    starting_point_url TEXT,            -- for git_repo
    starting_point_branch TEXT,         -- for git_repo

    -- Default configuration (inherited by runs)
    worker_scale TEXT,
    time_limit_minutes INTEGER,
    max_iterations INTEGER,
    human_in_the_loop INTEGER DEFAULT 1,

    -- Scribe/docs configuration
    docs_path TEXT DEFAULT 'docs',
    persist_docs_changes INTEGER DEFAULT 1,

    -- Metadata
    description TEXT
);

CREATE INDEX IF NOT EXISTS idx_projects_name ON projects(name);
"#;

/// Error type for project operations
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Project not found: {0}")]
    NotFound(String),
    #[error("Project already exists: {0}")]
    AlreadyExists(String),
}

pub type ProjectResult<T> = Result<T, ProjectError>;

/// Project store backed by global SQLite database
pub struct ProjectStore {
    db: Connection,
}

impl ProjectStore {
    /// Open the global project store
    pub fn open() -> ProjectResult<Self> {
        Self::open_at(&global_db_path())
    }

    /// Open from a specific path (useful for testing)
    pub fn open_at(path: &Path) -> ProjectResult<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        // Enable WAL mode for better concurrent read/write performance
        db.pragma_update(None, "journal_mode", "WAL")?;

        let store = Self { db };
        store.init_db()?;
        Ok(store)
    }

    fn init_db(&self) -> ProjectResult<()> {
        self.db.execute_batch(SCHEMA)?;
        Ok(())
    }

    fn now(&self) -> String {
        chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string()
    }

    /// Create a new project
    pub fn create_project(&self, req: &CreateProjectRequest) -> ProjectResult<Project> {
        // Check if project with this name already exists
        let exists: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE name = ?1)",
            params![&req.name],
            |row| row.get(0),
        )?;

        if exists {
            return Err(ProjectError::AlreadyExists(req.name.clone()));
        }

        let now = self.now();
        let (sp_type, sp_path, sp_url, sp_branch) =
            self.normalize_starting_point(&req.starting_point);

        let human_in_the_loop = req.human_in_the_loop.unwrap_or(true);
        let docs_path = req.docs_path.as_deref().unwrap_or("docs");
        let persist_docs_changes = req.persist_docs_changes.unwrap_or(true);

        self.db.execute(
            "INSERT INTO projects (
                name, created_at, updated_at,
                starting_point_type, starting_point_path, starting_point_url, starting_point_branch,
                worker_scale, time_limit_minutes, max_iterations, human_in_the_loop,
                docs_path, persist_docs_changes, description
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                &req.name,
                &now,
                &now,
                sp_type,
                sp_path,
                sp_url,
                sp_branch,
                req.worker_scale,
                req.time_limit_minutes,
                req.max_iterations,
                human_in_the_loop as i64,
                docs_path,
                persist_docs_changes as i64,
                req.description,
            ],
        )?;

        let id = self.db.last_insert_rowid();
        self.get_project(id)
    }

    /// Get a project by ID
    pub fn get_project(&self, id: i64) -> ProjectResult<Project> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, created_at, updated_at,
                    starting_point_type, starting_point_path, starting_point_url, starting_point_branch,
                    worker_scale, time_limit_minutes, max_iterations, human_in_the_loop,
                    docs_path, persist_docs_changes, description
             FROM projects
             WHERE id = ?1",
        )?;

        stmt.query_row([id], |row| self.row_to_project(row))
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => ProjectError::NotFound(id.to_string()),
                e => ProjectError::Database(e),
            })
    }

    /// Get a project by name
    pub fn get_project_by_name(&self, name: &str) -> ProjectResult<Option<Project>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, created_at, updated_at,
                    starting_point_type, starting_point_path, starting_point_url, starting_point_branch,
                    worker_scale, time_limit_minutes, max_iterations, human_in_the_loop,
                    docs_path, persist_docs_changes, description
             FROM projects
             WHERE name = ?1",
        )?;

        match stmt.query_row([name], |row| self.row_to_project(row)) {
            Ok(project) => Ok(Some(project)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ProjectError::Database(e)),
        }
    }

    /// List all projects
    pub fn list_projects(&self) -> ProjectResult<Vec<Project>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, created_at, updated_at,
                    starting_point_type, starting_point_path, starting_point_url, starting_point_branch,
                    worker_scale, time_limit_minutes, max_iterations, human_in_the_loop,
                    docs_path, persist_docs_changes, description
             FROM projects
             ORDER BY created_at DESC",
        )?;

        let projects = stmt
            .query_map([], |row| self.row_to_project(row))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    /// Update a project
    pub fn update_project(&self, id: i64, req: &UpdateProjectRequest) -> ProjectResult<Project> {
        // Check if project exists
        let _ = self.get_project(id)?;

        let now = self.now();

        // Build dynamic UPDATE query based on what's provided
        let mut updates = vec!["updated_at = ?1"];
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now.clone())];

        if let Some(ref sp) = req.starting_point {
            let (sp_type, sp_path, sp_url, sp_branch) = self.normalize_starting_point(sp);
            updates.push("starting_point_type = ?");
            updates.push("starting_point_path = ?");
            updates.push("starting_point_url = ?");
            updates.push("starting_point_branch = ?");
            values.push(Box::new(sp_type));
            values.push(Box::new(sp_path));
            values.push(Box::new(sp_url));
            values.push(Box::new(sp_branch));
        }

        if let Some(ref ws) = req.worker_scale {
            updates.push("worker_scale = ?");
            values.push(Box::new(ws.clone()));
        }

        if let Some(tl) = req.time_limit_minutes {
            updates.push("time_limit_minutes = ?");
            values.push(Box::new(tl));
        }

        if let Some(mi) = req.max_iterations {
            updates.push("max_iterations = ?");
            values.push(Box::new(mi));
        }

        if let Some(hitl) = req.human_in_the_loop {
            updates.push("human_in_the_loop = ?");
            values.push(Box::new(hitl as i64));
        }

        if let Some(ref dp) = req.docs_path {
            updates.push("docs_path = ?");
            values.push(Box::new(dp.clone()));
        }

        if let Some(pdc) = req.persist_docs_changes {
            updates.push("persist_docs_changes = ?");
            values.push(Box::new(pdc as i64));
        }

        if let Some(ref desc) = req.description {
            updates.push("description = ?");
            values.push(Box::new(desc.clone()));
        }

        values.push(Box::new(id));

        let sql = format!("UPDATE projects SET {} WHERE id = ?", updates.join(", "));

        // Convert to refs for execute
        let params: Vec<&dyn rusqlite::ToSql> = values.iter().map(|b| b.as_ref()).collect();

        self.db.execute(&sql, params.as_slice())?;

        self.get_project(id)
    }

    /// Delete a project (caller is responsible for cascading to runs)
    pub fn delete_project(&self, id: i64) -> ProjectResult<()> {
        // Check if project exists
        let _ = self.get_project(id)?;

        // Delete from database
        self.db
            .execute("DELETE FROM projects WHERE id = ?1", params![id])?;

        // Delete project data directory (board, etc.)
        let project_dir = crate::core::config::hirsel_dir()
            .join("projects")
            .join(id.to_string());
        if project_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&project_dir) {
                tracing::warn!(
                    "Failed to delete project directory {:?}: {}",
                    project_dir,
                    e
                );
            }
        }

        Ok(())
    }

    /// Normalize StartingPoint to separate columns
    fn normalize_starting_point(
        &self,
        sp: &StartingPoint,
    ) -> (String, Option<String>, Option<String>, Option<String>) {
        match sp {
            StartingPoint::Greenfield => ("greenfield".to_string(), None, None, None),
            StartingPoint::LocalFolder { path } => {
                ("local_folder".to_string(), Some(path.clone()), None, None)
            }
            StartingPoint::GitRepo { url, branch } => (
                "git_repo".to_string(),
                None,
                Some(url.clone()),
                branch.clone(),
            ),
        }
    }

    /// Denormalize database row to StartingPoint
    fn denormalize_starting_point(&self, row: &Row) -> rusqlite::Result<StartingPoint> {
        let sp_type: String = row.get("starting_point_type")?;
        match sp_type.as_str() {
            "greenfield" => Ok(StartingPoint::Greenfield),
            "local_folder" => {
                let path: String = row.get("starting_point_path")?;
                Ok(StartingPoint::LocalFolder { path })
            }
            "git_repo" => {
                let url: String = row.get("starting_point_url")?;
                let branch: Option<String> = row.get("starting_point_branch")?;
                Ok(StartingPoint::GitRepo { url, branch })
            }
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }

    /// Convert database row to Project
    fn row_to_project(&self, row: &Row) -> rusqlite::Result<Project> {
        Ok(Project {
            id: row.get("id")?,
            name: row.get("name")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            starting_point: self.denormalize_starting_point(row)?,
            worker_scale: row.get("worker_scale")?,
            time_limit_minutes: row.get("time_limit_minutes")?,
            max_iterations: row.get("max_iterations")?,
            human_in_the_loop: row.get::<_, i64>("human_in_the_loop")? != 0,
            docs_path: row.get("docs_path")?,
            persist_docs_changes: row.get::<_, i64>("persist_docs_changes")? != 0,
            description: row.get("description")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_create_and_get_project() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ProjectStore::open_at(&db_path).unwrap();

        let req = CreateProjectRequest {
            name: "test-project".to_string(),
            starting_point: StartingPoint::Greenfield,
            worker_scale: Some("2".to_string()),
            time_limit_minutes: Some(60),
            max_iterations: None,
            human_in_the_loop: Some(true),
            docs_path: Some("docs".to_string()),
            persist_docs_changes: Some(true),
            description: Some("Test project".to_string()),
        };

        let project = store.create_project(&req).unwrap();
        assert_eq!(project.name, "test-project");
        assert_eq!(project.worker_scale, Some("2".to_string()));
        assert_eq!(project.time_limit_minutes, Some(60));

        // Get by ID
        let loaded = store.get_project(project.id).unwrap();
        assert_eq!(loaded.name, project.name);

        // Get by name
        let by_name = store.get_project_by_name("test-project").unwrap();
        assert!(by_name.is_some());
        assert_eq!(by_name.unwrap().id, project.id);
    }

    #[test]
    fn test_list_projects() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ProjectStore::open_at(&db_path).unwrap();

        store
            .create_project(&CreateProjectRequest {
                name: "project1".to_string(),
                starting_point: StartingPoint::Greenfield,
                worker_scale: None,
                time_limit_minutes: None,
                max_iterations: None,
                human_in_the_loop: None,
                docs_path: None,
                persist_docs_changes: None,
                description: None,
            })
            .unwrap();

        store
            .create_project(&CreateProjectRequest {
                name: "project2".to_string(),
                starting_point: StartingPoint::GitRepo {
                    url: "https://github.com/test/repo".to_string(),
                    branch: Some("main".to_string()),
                },
                worker_scale: None,
                time_limit_minutes: None,
                max_iterations: None,
                human_in_the_loop: None,
                docs_path: None,
                persist_docs_changes: None,
                description: None,
            })
            .unwrap();

        let projects = store.list_projects().unwrap();
        assert_eq!(projects.len(), 2);
    }

    #[test]
    fn test_update_project() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ProjectStore::open_at(&db_path).unwrap();

        let project = store
            .create_project(&CreateProjectRequest {
                name: "test".to_string(),
                starting_point: StartingPoint::Greenfield,
                worker_scale: None,
                time_limit_minutes: None,
                max_iterations: None,
                human_in_the_loop: None,
                docs_path: None,
                persist_docs_changes: None,
                description: None,
            })
            .unwrap();

        let updated = store
            .update_project(
                project.id,
                &UpdateProjectRequest {
                    starting_point: None,
                    worker_scale: Some("4".to_string()),
                    time_limit_minutes: Some(120),
                    max_iterations: None,
                    human_in_the_loop: None,
                    docs_path: None,
                    persist_docs_changes: None,
                    description: Some("Updated".to_string()),
                },
            )
            .unwrap();

        assert_eq!(updated.worker_scale, Some("4".to_string()));
        assert_eq!(updated.time_limit_minutes, Some(120));
        assert_eq!(updated.description, Some("Updated".to_string()));
    }

    #[test]
    fn test_delete_project() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ProjectStore::open_at(&db_path).unwrap();

        let project = store
            .create_project(&CreateProjectRequest {
                name: "test".to_string(),
                starting_point: StartingPoint::Greenfield,
                worker_scale: None,
                time_limit_minutes: None,
                max_iterations: None,
                human_in_the_loop: None,
                docs_path: None,
                persist_docs_changes: None,
                description: None,
            })
            .unwrap();

        store.delete_project(project.id).unwrap();

        assert!(store.get_project(project.id).is_err());
    }

    #[test]
    fn test_starting_point_normalization() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let store = ProjectStore::open_at(&db_path).unwrap();

        // Test Greenfield
        let p1 = store
            .create_project(&CreateProjectRequest {
                name: "greenfield".to_string(),
                starting_point: StartingPoint::Greenfield,
                worker_scale: None,
                time_limit_minutes: None,
                max_iterations: None,
                human_in_the_loop: None,
                docs_path: None,
                persist_docs_changes: None,
                description: None,
            })
            .unwrap();
        assert!(matches!(p1.starting_point, StartingPoint::Greenfield));

        // Test LocalFolder
        let p2 = store
            .create_project(&CreateProjectRequest {
                name: "local".to_string(),
                starting_point: StartingPoint::LocalFolder {
                    path: "/tmp/test".to_string(),
                },
                worker_scale: None,
                time_limit_minutes: None,
                max_iterations: None,
                human_in_the_loop: None,
                docs_path: None,
                persist_docs_changes: None,
                description: None,
            })
            .unwrap();
        if let StartingPoint::LocalFolder { path } = p2.starting_point {
            assert_eq!(path, "/tmp/test");
        } else {
            panic!("Expected LocalFolder");
        }

        // Test GitRepo
        let p3 = store
            .create_project(&CreateProjectRequest {
                name: "git".to_string(),
                starting_point: StartingPoint::GitRepo {
                    url: "https://github.com/test/repo".to_string(),
                    branch: Some("main".to_string()),
                },
                worker_scale: None,
                time_limit_minutes: None,
                max_iterations: None,
                human_in_the_loop: None,
                docs_path: None,
                persist_docs_changes: None,
                description: None,
            })
            .unwrap();
        if let StartingPoint::GitRepo { url, branch } = p3.starting_point {
            assert_eq!(url, "https://github.com/test/repo");
            assert_eq!(branch, Some("main".to_string()));
        } else {
            panic!("Expected GitRepo");
        }
    }
}
