//! Route file management
//!
//! Handles the file structure for routes:
//! ```
//! ~/.hirsel/projects/{project_id}/routes/
//! ├── main/
//! │   ├── docs/
//! │   ├── board.md
//! │   └── code/
//! └── feature-v1/
//!     ├── docs/
//!     ├── board.md
//!     └── code/
//! ```

use std::path::PathBuf;

use crate::core::config::hirsel_dir;

/// File management for a route
pub struct RouteFiles {
    project_id: i64,
    route_name: String,
}

impl RouteFiles {
    /// Create a new RouteFiles instance
    pub fn new(project_id: i64, route_name: &str) -> Self {
        Self {
            project_id,
            route_name: route_name.to_string(),
        }
    }

    /// Get the base routes directory for a project
    pub fn routes_base_dir(project_id: i64) -> PathBuf {
        hirsel_dir()
            .join("projects")
            .join(project_id.to_string())
            .join("routes")
    }

    /// Get the route directory
    pub fn route_dir(&self) -> PathBuf {
        Self::routes_base_dir(self.project_id).join(&self.route_name)
    }

    /// Get the docs directory for this route
    pub fn docs_dir(&self) -> PathBuf {
        self.route_dir().join("docs")
    }

    /// Get the board directory for this route
    pub fn board_dir(&self) -> PathBuf {
        self.route_dir().join("board")
    }

    /// Get the board tasks directory (for content files)
    pub fn board_tasks_dir(&self) -> PathBuf {
        self.board_dir().join("tasks")
    }

    /// Get the board.md file path
    pub fn board_md(&self) -> PathBuf {
        self.route_dir().join("board.md")
    }

    /// Get the code snapshot directory
    pub fn code_dir(&self) -> PathBuf {
        self.route_dir().join("code")
    }

    /// Initialize the route directory structure
    pub fn init_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.docs_dir())?;
        std::fs::create_dir_all(self.board_tasks_dir())?;
        std::fs::create_dir_all(self.code_dir())?;
        Ok(())
    }

    /// Check if the route directory exists
    pub fn exists(&self) -> bool {
        self.route_dir().exists()
    }

    /// Copy docs from another route
    pub fn copy_docs_from(&self, source: &RouteFiles) -> std::io::Result<()> {
        let source_docs = source.docs_dir();
        let target_docs = self.docs_dir();

        if !source_docs.exists() {
            return Ok(());
        }

        // Create target directory
        std::fs::create_dir_all(&target_docs)?;

        // Copy all files
        copy_dir_recursive(&source_docs, &target_docs)?;

        Ok(())
    }

    /// Copy board content files from another route
    pub fn copy_board_from(&self, source: &RouteFiles) -> std::io::Result<()> {
        let source_board = source.board_tasks_dir();
        let target_board = self.board_tasks_dir();

        if !source_board.exists() {
            return Ok(());
        }

        // Create target directory
        std::fs::create_dir_all(&target_board)?;

        // Copy all files
        copy_dir_recursive(&source_board, &target_board)?;

        Ok(())
    }

    /// Copy code snapshot from another route
    pub fn copy_code_from(&self, source: &RouteFiles) -> std::io::Result<()> {
        let source_code = source.code_dir();
        let target_code = self.code_dir();

        if !source_code.exists() {
            return Ok(());
        }

        // Create target directory
        std::fs::create_dir_all(&target_code)?;

        // Copy all files
        copy_dir_recursive(&source_code, &target_code)?;

        Ok(())
    }

    /// Delete the route directory
    pub fn delete(&self) -> std::io::Result<()> {
        let dir = self.route_dir();
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}
