//! File system utilities for hirsel run directories.
//!
//! This module provides path accessors and file operations for the
//! run directory structure, including spec files, tasks, chats, and logs.
//!
//! ## Storage Abstraction
//!
//! The `Files` struct provides path helpers plus async methods that work with the
//! `FileStorage` trait for storage-agnostic operations. Prefer async methods for
//! all I/O in new code.
//!
//! ```rust,ignore
//! // Local filesystem (default)
//! let files = Files::new("/path/to/run");
//! let storage = create_default_local_storage();
//! files.write_spec_async(&storage, "# My Spec").await?;
//!
//! // With storage abstraction
//! let storage = create_file_storage(&config.storage).await?;
//! files.write_spec_async(&storage, "# My Spec").await?; // async, works with S3 too
//! ```

use chrono::Local;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use super::storage::{FileStorage, StorageResult};

/// Regex pattern for parsing task table rows: | id | STATUS | worker | name |
static TASK_ROW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\| ([a-z_][a-z0-9_]*) \| (TODO|DOING|DONE) \| ([^|]*)\| (.+) \|$").unwrap()
});

/// Represents a task parsed from tasks.md
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedTask {
    pub id: String,
    pub status: String,
    pub claimed_by: Option<String>,
    pub name: String,
}

/// File system utilities for a hirsel run directory.
///
/// Provides access to all standard paths within a run directory
/// and operations for managing tasks, logs, and chats.
#[derive(Debug, Clone)]
pub struct Files {
    run_dir: PathBuf,
}

impl Files {
    /// Create a new Files instance for the given run directory.
    pub fn new<P: AsRef<Path>>(run_dir: P) -> Self {
        Self {
            run_dir: run_dir.as_ref().to_path_buf(),
        }
    }

    /// Get the run directory path.
    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// Get the run name (extracted from the run directory path).
    pub fn run_name(&self) -> Option<String> {
        self.run_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(|s| s.to_string())
    }

    /// Path to spec.md - the run specification file.
    pub fn spec(&self) -> PathBuf {
        self.run_dir.join("spec.md")
    }

    /// Path to tasks.md - the markdown task table.
    pub fn tasks_md(&self) -> PathBuf {
        self.run_dir.join("tasks.md")
    }

    /// Path to the tasks/ directory containing task detail files.
    pub fn tasks_dir(&self) -> PathBuf {
        self.run_dir.join("tasks")
    }

    /// Path to log.md - the activity log file.
    pub fn log(&self) -> PathBuf {
        self.run_dir.join("log.md")
    }

    /// Path to eval.md - the evaluation specification.
    pub fn eval_spec(&self) -> PathBuf {
        self.run_dir.join("eval.md")
    }

    /// Path to assets/ directory - contains images and other assets for spec/eval.
    pub fn assets(&self) -> PathBuf {
        self.run_dir.join("assets")
    }

    /// Path to tmp/eval_log.md - the evaluation log.
    pub fn eval_log(&self) -> PathBuf {
        self.run_dir.join("tmp").join("eval_log.md")
    }

    /// Path to work/ directory - contains git worktrees for workers.
    pub fn work(&self) -> PathBuf {
        self.run_dir.join("work")
    }

    /// Path to chats/ directory - contains chat thread files.
    pub fn chats_dir(&self) -> PathBuf {
        self.run_dir.join("chats")
    }

    /// Path to a worker's log file in tmp/.
    pub fn worker_log(&self, worker_name: &str) -> PathBuf {
        self.run_dir
            .join("tmp")
            .join(format!("{}.log", worker_name))
    }

    /// Path to a chat thread file.
    pub fn chat_file(&self, name: &str) -> PathBuf {
        self.chats_dir().join(format!("{}.md", name))
    }

    /// Path to a task detail file.
    pub fn task_detail(&self, task_id: &str) -> PathBuf {
        self.tasks_dir().join(format!("{}.md", task_id))
    }

    /// Path to the SQLite database file.
    pub fn db_path(&self) -> PathBuf {
        self.run_dir.join("hirsel.db")
    }

    /// Initialize all required directories for a run.
    ///
    /// Creates:
    /// - run_dir
    /// - tasks/
    /// - chats/
    /// - tmp/
    pub fn init_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(&self.run_dir)?;
        fs::create_dir_all(self.tasks_dir())?;
        fs::create_dir_all(self.chats_dir())?;
        fs::create_dir_all(self.run_dir.join("tmp"))?;
        Ok(())
    }

    /// Append a timestamped message to the activity log.
    ///
    /// Format: "HH:MM message\n"
    pub fn append_log(&self, message: &str) -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log())?;
        let timestamp = Local::now().format("%H:%M");
        writeln!(file, "{} {}", timestamp, message)?;
        Ok(())
    }

    /// Initialize tasks.md with the header if it doesn't exist.
    pub fn init_tasks_md(&self) -> io::Result<()> {
        let tasks_md = self.tasks_md();
        if !tasks_md.exists() {
            let header =
                "# Tasks\n\n| ID | Status | Worker | Name |\n|----|--------|--------|------|\n";
            fs::write(&tasks_md, header)?;
        }
        Ok(())
    }

    /// Render a list of tasks as a markdown table.
    pub fn render_tasks_table(&self, tasks: &[ParsedTask]) -> String {
        let mut lines = vec![
            "# Tasks".to_string(),
            String::new(),
            "| ID | Status | Worker | Name |".to_string(),
            "|----|--------|--------|------|".to_string(),
        ];

        for task in tasks {
            let worker = task.claimed_by.as_deref().unwrap_or("");
            lines.push(format!(
                "| {} | {} | {} | {} |",
                task.id,
                task.status.to_uppercase(),
                worker,
                task.name
            ));
        }
        lines.push(String::new());
        lines.join("\n")
    }

    /// Update tasks.md with the given task list.
    pub fn update_tasks_md(&self, tasks: &[ParsedTask]) -> io::Result<()> {
        let content = self.render_tasks_table(tasks);
        fs::write(self.tasks_md(), content)
    }

    /// Create a task detail file if it doesn't exist.
    pub fn create_task_detail(&self, task_id: &str, name: &str) -> io::Result<()> {
        let detail_path = self.task_detail(task_id);
        if !detail_path.exists() {
            fs::write(&detail_path, format!("# {}\n\n", name))?;
        }
        Ok(())
    }

    /// Write content to a task detail file (overwrite if exists).
    pub fn write_task_detail(&self, task_id: &str, content: &str) -> io::Result<()> {
        fs::write(self.task_detail(task_id), content)
    }

    /// Read and return task detail content, if the file exists.
    pub fn read_task_detail(&self, task_id: &str) -> io::Result<Option<String>> {
        let path = self.task_detail(task_id);
        if path.exists() {
            Ok(Some(fs::read_to_string(path)?))
        } else {
            Ok(None)
        }
    }

    /// Parse tasks.md and return a list of tasks.
    pub fn parse_tasks_md(&self) -> io::Result<Vec<ParsedTask>> {
        let tasks_md = self.tasks_md();
        if !tasks_md.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&tasks_md)?;
        let mut tasks = Vec::new();

        for line in content.lines() {
            if let Some(captures) = TASK_ROW_RE.captures(line.trim()) {
                let claimed_by = captures
                    .get(3)
                    .map(|m| m.as_str().trim())
                    .filter(|s| !s.is_empty())
                    .map(String::from);

                tasks.push(ParsedTask {
                    id: captures[1].to_string(),
                    status: captures[2].to_lowercase(),
                    claimed_by,
                    name: captures[4].trim().to_string(),
                });
            }
        }

        Ok(tasks)
    }

    /// Ensure task detail files exist for all tasks in tasks.md.
    pub fn ensure_task_details(&self) -> io::Result<()> {
        let tasks = self.parse_tasks_md()?;
        for task in tasks {
            self.create_task_detail(&task.id, &task.name)?;
        }
        Ok(())
    }

    /// Read spec.md content if it exists.
    pub fn read_spec(&self) -> io::Result<Option<String>> {
        let path = self.spec();
        if path.exists() {
            Ok(Some(fs::read_to_string(path)?))
        } else {
            Ok(None)
        }
    }

    /// Write content to spec.md.
    pub fn write_spec(&self, content: &str) -> io::Result<()> {
        fs::write(self.spec(), content)
    }

    /// Read log.md content if it exists.
    pub fn read_log(&self) -> io::Result<Option<String>> {
        let path = self.log();
        if path.exists() {
            Ok(Some(fs::read_to_string(path)?))
        } else {
            Ok(None)
        }
    }

    /// List all chat thread names (without .md extension).
    pub fn list_chat_threads(&self) -> io::Result<Vec<String>> {
        let chats_dir = self.chats_dir();
        if !chats_dir.exists() {
            return Ok(Vec::new());
        }

        let mut threads = Vec::new();
        for entry in fs::read_dir(&chats_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                if let Some(stem) = path.file_stem() {
                    threads.push(stem.to_string_lossy().to_string());
                }
            }
        }
        threads.sort();
        Ok(threads)
    }

    /// List all worker log files and return their names.
    pub fn list_worker_logs(&self) -> io::Result<Vec<String>> {
        let tmp_dir = self.run_dir.join("tmp");
        if !tmp_dir.exists() {
            return Ok(Vec::new());
        }

        let mut logs = Vec::new();
        for entry in fs::read_dir(&tmp_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "log").unwrap_or(false) {
                if let Some(stem) = path.file_stem() {
                    logs.push(stem.to_string_lossy().to_string());
                }
            }
        }
        logs.sort();
        Ok(logs)
    }

    // =========================================================================
    // Async methods using FileStorage trait
    // =========================================================================
    // These methods work with any FileStorage implementation (local, S3, etc.)

    /// Get the storage path prefix for this run (relative to storage root).
    ///
    /// For a run named "myrun", this returns "runs/myrun".
    /// Use this when constructing paths for FileStorage operations.
    pub fn storage_prefix(&self) -> String {
        // Extract run name from run_dir path
        if let Some(name) = self.run_dir.file_name() {
            format!("runs/{}", name.to_string_lossy())
        } else {
            "runs".to_string()
        }
    }

    /// Get the storage path for a file relative to this run.
    pub fn storage_path(&self, relative: &str) -> String {
        format!("{}/{}", self.storage_prefix(), relative)
    }

    /// Read spec.md using FileStorage.
    pub async fn read_spec_async(
        &self,
        storage: &dyn FileStorage,
    ) -> StorageResult<Option<String>> {
        let path = self.storage_path("spec.md");
        match storage.read_string(&path).await {
            Ok(content) => Ok(Some(content)),
            Err(super::storage::StorageError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Write spec.md using FileStorage.
    pub async fn write_spec_async(
        &self,
        storage: &dyn FileStorage,
        content: &str,
    ) -> StorageResult<()> {
        let path = self.storage_path("spec.md");
        storage.write_string(&path, content).await
    }

    /// Read eval.md using FileStorage.
    pub async fn read_eval_async(
        &self,
        storage: &dyn FileStorage,
    ) -> StorageResult<Option<String>> {
        let path = self.storage_path("eval.md");
        match storage.read_string(&path).await {
            Ok(content) => Ok(Some(content)),
            Err(super::storage::StorageError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Write eval.md using FileStorage.
    pub async fn write_eval_async(
        &self,
        storage: &dyn FileStorage,
        content: &str,
    ) -> StorageResult<()> {
        let path = self.storage_path("eval.md");
        storage.write_string(&path, content).await
    }

    /// Read tasks.md using FileStorage.
    pub async fn read_tasks_md_async(
        &self,
        storage: &dyn FileStorage,
    ) -> StorageResult<Option<String>> {
        let path = self.storage_path("tasks.md");
        match storage.read_string(&path).await {
            Ok(content) => Ok(Some(content)),
            Err(super::storage::StorageError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Write tasks.md using FileStorage.
    pub async fn write_tasks_md_async(
        &self,
        storage: &dyn FileStorage,
        content: &str,
    ) -> StorageResult<()> {
        let path = self.storage_path("tasks.md");
        storage.write_string(&path, content).await
    }

    /// Initialize directories using FileStorage.
    pub async fn init_dirs_async(&self, storage: &dyn FileStorage) -> StorageResult<()> {
        storage.create_dir(&self.storage_prefix()).await?;
        storage.create_dir(&self.storage_path("tasks")).await?;
        storage.create_dir(&self.storage_path("chats")).await?;
        storage.create_dir(&self.storage_path("tmp")).await?;
        Ok(())
    }

    /// Read a task detail file using FileStorage.
    pub async fn read_task_detail_async(
        &self,
        storage: &dyn FileStorage,
        task_id: &str,
    ) -> StorageResult<Option<String>> {
        let path = self.storage_path(&format!("tasks/{}.md", task_id));
        match storage.read_string(&path).await {
            Ok(content) => Ok(Some(content)),
            Err(super::storage::StorageError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Write a task detail file using FileStorage.
    pub async fn write_task_detail_async(
        &self,
        storage: &dyn FileStorage,
        task_id: &str,
        content: &str,
    ) -> StorageResult<()> {
        let path = self.storage_path(&format!("tasks/{}.md", task_id));
        storage.write_string(&path, content).await
    }

    /// Read log.md using FileStorage.
    pub async fn read_log_async(&self, storage: &dyn FileStorage) -> StorageResult<Option<String>> {
        let path = self.storage_path("log.md");
        match storage.read_string(&path).await {
            Ok(content) => Ok(Some(content)),
            Err(super::storage::StorageError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Append to log.md using FileStorage.
    ///
    /// Note: For S3, this reads and rewrites the entire file.
    pub async fn append_log_async(
        &self,
        storage: &dyn FileStorage,
        message: &str,
    ) -> StorageResult<()> {
        let path = self.storage_path("log.md");
        let timestamp = Local::now().format("%H:%M");
        let line = format!("{} {}\n", timestamp, message);

        // Read existing content, append, write back
        let existing = match storage.read_string(&path).await {
            Ok(content) => content,
            Err(super::storage::StorageError::NotFound(_)) => String::new(),
            Err(e) => return Err(e),
        };

        let new_content = format!("{}{}", existing, line);
        storage.write_string(&path, &new_content).await
    }

    /// Read worker log using FileStorage.
    pub async fn read_worker_log_async(
        &self,
        storage: &dyn FileStorage,
        worker_name: &str,
    ) -> StorageResult<Option<String>> {
        let path = self.storage_path(&format!("tmp/{}.log", worker_name));
        match storage.read_string(&path).await {
            Ok(content) => Ok(Some(content)),
            Err(super::storage::StorageError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Write raw bytes to a file using FileStorage.
    pub async fn write_bytes_async(
        &self,
        storage: &dyn FileStorage,
        relative_path: &str,
        data: &[u8],
    ) -> StorageResult<()> {
        let path = self.storage_path(relative_path);
        storage.write(&path, data).await
    }

    /// Read raw bytes from a file using FileStorage.
    pub async fn read_bytes_async(
        &self,
        storage: &dyn FileStorage,
        relative_path: &str,
    ) -> StorageResult<Vec<u8>> {
        let path = self.storage_path(relative_path);
        storage.read(&path).await
    }

    /// Check if a file exists using FileStorage.
    pub async fn exists_async(
        &self,
        storage: &dyn FileStorage,
        relative_path: &str,
    ) -> StorageResult<bool> {
        let path = self.storage_path(relative_path);
        storage.exists(&path).await
    }

    /// Delete a file using FileStorage.
    pub async fn delete_async(
        &self,
        storage: &dyn FileStorage,
        relative_path: &str,
    ) -> StorageResult<()> {
        let path = self.storage_path(relative_path);
        storage.delete(&path).await
    }

    /// List files in a directory using FileStorage.
    pub async fn list_async(
        &self,
        storage: &dyn FileStorage,
        relative_prefix: &str,
    ) -> StorageResult<Vec<String>> {
        let prefix = self.storage_path(relative_prefix);
        let files = storage.list(&prefix).await?;
        // Strip the run prefix from results to get relative paths
        let run_prefix = format!("{}/", self.storage_prefix());
        Ok(files
            .into_iter()
            .filter_map(|f| f.strip_prefix(&run_prefix).map(String::from))
            .collect())
    }
}

// =========================================================================
// Docs Directory Methods
// =========================================================================

/// A documentation file from the docs/ directory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocFile {
    pub name: String,
    pub content: String,
}

/// Documentation content - either a single file or all files
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DocsContent {
    Single { name: String, content: String },
    All { files: Vec<DocFile> },
}

impl Files {
    /// Path to the docs/ directory - contains project documentation.
    pub fn docs_dir(&self) -> PathBuf {
        self.run_dir.join("docs")
    }

    /// Initialize docs/ using FileStorage.
    pub async fn init_docs_async(&self, storage: &dyn FileStorage) -> StorageResult<()> {
        let docs_prefix = self.storage_path("docs");
        storage.create_dir(&docs_prefix).await?;

        let defaults = [
            (
                "architecture.md",
                "# Architecture\n\nSystem design and module relationships.\n",
            ),
            (
                "patterns.md",
                "# Patterns\n\nCode patterns and conventions.\n",
            ),
            (
                "gotchas.md",
                "# Gotchas\n\nPitfalls and things to watch out for.\n",
            ),
            (
                "decisions.md",
                "# Decisions\n\nKey decisions and rationale.\n",
            ),
        ];

        for (name, content) in defaults {
            let path = self.storage_path(&format!("docs/{}", name));
            if !storage.exists(&path).await? {
                storage.write_string(&path, content).await?;
            }
        }

        Ok(())
    }

    /// Read docs/ using FileStorage.
    pub async fn read_docs_async(
        &self,
        storage: &dyn FileStorage,
        file: Option<&str>,
    ) -> StorageResult<DocsContent> {
        if let Some(name) = file {
            let path = self.storage_path(&format!("docs/{}", name));
            let content = storage.read_string(&path).await?;
            return Ok(DocsContent::Single {
                name: name.to_string(),
                content,
            });
        }

        let docs_prefix = self.storage_path("docs/");
        let mut files = Vec::new();
        for path in storage.list(&docs_prefix).await? {
            if !path.ends_with(".md") {
                continue;
            }

            let name = path
                .strip_prefix(&docs_prefix)
                .unwrap_or(&path)
                .trim_start_matches('/')
                .to_string();

            let content = storage.read_string(&path).await?;
            files.push(DocFile { name, content });
        }
        files.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(DocsContent::All { files })
    }

    /// Compute docs hashes using FileStorage.
    pub async fn get_docs_hashes_async(
        &self,
        storage: &dyn FileStorage,
    ) -> StorageResult<std::collections::HashMap<String, String>> {
        use std::collections::HashMap;
        use std::hash::{Hash, Hasher};

        let docs_prefix = self.storage_path("docs/");
        let mut hashes = HashMap::new();

        for path in storage.list(&docs_prefix).await? {
            if !path.ends_with(".md") {
                continue;
            }

            let name = path
                .strip_prefix(&docs_prefix)
                .unwrap_or(&path)
                .trim_start_matches('/')
                .to_string();

            let content = storage.read_string(&path).await?;
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            content.hash(&mut hasher);
            hashes.insert(name, format!("{:016x}", hasher.finish()));
        }

        Ok(hashes)
    }

    /// Initialize the docs/ directory with default documentation files.
    ///
    /// Creates the docs directory and default markdown files if they don't exist:
    /// - architecture.md - System design and module relationships
    /// - patterns.md - Code patterns and conventions
    /// - gotchas.md - Pitfalls and things to watch out for
    /// - decisions.md - Key decisions and rationale
    pub fn init_docs(&self) -> io::Result<()> {
        let dir = self.docs_dir();
        fs::create_dir_all(&dir)?;

        let defaults = [
            (
                "architecture.md",
                "# Architecture\n\nSystem design and module relationships.\n",
            ),
            (
                "patterns.md",
                "# Patterns\n\nCode patterns and conventions.\n",
            ),
            (
                "gotchas.md",
                "# Gotchas\n\nPitfalls and things to watch out for.\n",
            ),
            (
                "decisions.md",
                "# Decisions\n\nKey decisions and rationale.\n",
            ),
        ];

        for (name, content) in defaults {
            let path = dir.join(name);
            if !path.exists() {
                fs::write(&path, content)?;
            }
        }
        Ok(())
    }

    /// Read documentation files from docs/.
    ///
    /// If `file` is Some, reads that specific file.
    /// If `file` is None, reads all markdown files in the docs directory.
    pub fn read_docs(&self, file: Option<&str>) -> io::Result<DocsContent> {
        let dir = self.docs_dir();
        if let Some(name) = file {
            let content = fs::read_to_string(dir.join(name))?;
            Ok(DocsContent::Single {
                name: name.to_string(),
                content,
            })
        } else {
            let mut files = Vec::new();
            if dir.exists() {
                for entry in fs::read_dir(&dir)? {
                    let entry = entry?;
                    if entry.path().extension().is_some_and(|e| e == "md") {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let content = fs::read_to_string(entry.path())?;
                        files.push(DocFile { name, content });
                    }
                }
            }
            // Sort by name for consistent ordering
            files.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(DocsContent::All { files })
        }
    }

    /// Compute hashes for all doc files.
    ///
    /// Returns a map of filename -> SHA256 hash for sync purposes.
    pub fn get_docs_hashes(&self) -> io::Result<std::collections::HashMap<String, String>> {
        use std::collections::HashMap;

        let dir = self.docs_dir();
        let mut hashes = HashMap::new();

        if !dir.exists() {
            return Ok(hashes);
        }

        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|e| e == "md") {
                let name = entry.file_name().to_string_lossy().to_string();
                let content = fs::read_to_string(entry.path())?;

                // Compute simple hash using a fast hash function
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                content.hash(&mut hasher);
                let hash = format!("{:016x}", hasher.finish());

                hashes.insert(name, hash);
            }
        }

        Ok(hashes)
    }
}

/// Create a Files instance for the given run directory.
///
/// This is a convenience function matching the Python API.
pub fn get_files<P: AsRef<Path>>(run_dir: P) -> Files {
    Files::new(run_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_paths() {
        let files = Files::new("/test/run");
        assert_eq!(files.spec(), PathBuf::from("/test/run/spec.md"));
        assert_eq!(files.tasks_md(), PathBuf::from("/test/run/tasks.md"));
        assert_eq!(files.tasks_dir(), PathBuf::from("/test/run/tasks"));
        assert_eq!(files.log(), PathBuf::from("/test/run/log.md"));
        assert_eq!(files.work(), PathBuf::from("/test/run/work"));
        assert_eq!(files.chats_dir(), PathBuf::from("/test/run/chats"));
        assert_eq!(files.db_path(), PathBuf::from("/test/run/hirsel.db"));
    }

    #[test]
    fn test_worker_log_path() {
        let files = Files::new("/test/run");
        assert_eq!(
            files.worker_log("alpha"),
            PathBuf::from("/test/run/tmp/alpha.log")
        );
    }

    #[test]
    fn test_chat_file_path() {
        let files = Files::new("/test/run");
        assert_eq!(
            files.chat_file("user"),
            PathBuf::from("/test/run/chats/user.md")
        );
    }

    #[test]
    fn test_task_detail_path() {
        let files = Files::new("/test/run");
        assert_eq!(
            files.task_detail("implement_auth"),
            PathBuf::from("/test/run/tasks/implement_auth.md")
        );
    }

    #[test]
    fn test_init_dirs() {
        let temp = TempDir::new().unwrap();
        let files = Files::new(temp.path().join("myrun"));

        files.init_dirs().unwrap();

        assert!(files.run_dir().exists());
        assert!(files.tasks_dir().exists());
        assert!(files.chats_dir().exists());
        assert!(temp.path().join("myrun/tmp").exists());
    }

    #[test]
    fn test_init_tasks_md() {
        let temp = TempDir::new().unwrap();
        let files = Files::new(temp.path());

        files.init_tasks_md().unwrap();

        let content = fs::read_to_string(files.tasks_md()).unwrap();
        assert!(content.contains("# Tasks"));
        assert!(content.contains("| ID | Status | Worker | Name |"));
    }

    #[test]
    fn test_append_log() {
        let temp = TempDir::new().unwrap();
        let files = Files::new(temp.path());

        files.append_log("alpha claimed scope").unwrap();
        files.append_log("alpha completed scope").unwrap();

        let content = fs::read_to_string(files.log()).unwrap();
        assert!(content.contains("alpha claimed scope"));
        assert!(content.contains("alpha completed scope"));
    }

    #[test]
    fn test_render_tasks_table() {
        let files = Files::new("/test");
        let tasks = vec![
            ParsedTask {
                id: "scope".to_string(),
                status: "done".to_string(),
                claimed_by: Some("alpha".to_string()),
                name: "Define project scope".to_string(),
            },
            ParsedTask {
                id: "implement".to_string(),
                status: "doing".to_string(),
                claimed_by: Some("beta".to_string()),
                name: "Implement feature".to_string(),
            },
            ParsedTask {
                id: "test".to_string(),
                status: "todo".to_string(),
                claimed_by: None,
                name: "Write tests".to_string(),
            },
        ];

        let table = files.render_tasks_table(&tasks);

        assert!(table.contains("# Tasks"));
        assert!(table.contains("| scope | DONE | alpha | Define project scope |"));
        assert!(table.contains("| implement | DOING | beta | Implement feature |"));
        assert!(table.contains("| test | TODO |  | Write tests |"));
    }

    #[test]
    fn test_parse_tasks_md() {
        let temp = TempDir::new().unwrap();
        let files = Files::new(temp.path());

        let content = r#"# Tasks

| ID | Status | Worker | Name |
|----|--------|--------|------|
| scope | DONE | alpha | Define scope |
| implement | DOING | beta | Implement feature |
| review | TODO | | Code review |
"#;
        fs::write(files.tasks_md(), content).unwrap();

        let tasks = files.parse_tasks_md().unwrap();

        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].id, "scope");
        assert_eq!(tasks[0].status, "done");
        assert_eq!(tasks[0].claimed_by, Some("alpha".to_string()));
        assert_eq!(tasks[0].name, "Define scope");

        assert_eq!(tasks[1].id, "implement");
        assert_eq!(tasks[1].status, "doing");
        assert_eq!(tasks[1].claimed_by, Some("beta".to_string()));

        assert_eq!(tasks[2].id, "review");
        assert_eq!(tasks[2].status, "todo");
        assert_eq!(tasks[2].claimed_by, None);
    }

    #[test]
    fn test_create_task_detail() {
        let temp = TempDir::new().unwrap();
        let files = Files::new(temp.path());
        fs::create_dir_all(files.tasks_dir()).unwrap();

        files.create_task_detail("my_task", "My Task Name").unwrap();

        let content = fs::read_to_string(files.task_detail("my_task")).unwrap();
        assert_eq!(content, "# My Task Name\n\n");

        // Should not overwrite existing
        fs::write(files.task_detail("my_task"), "modified content").unwrap();
        files
            .create_task_detail("my_task", "Different Name")
            .unwrap();
        let content = fs::read_to_string(files.task_detail("my_task")).unwrap();
        assert_eq!(content, "modified content");
    }

    #[test]
    fn test_update_tasks_md() {
        let temp = TempDir::new().unwrap();
        let files = Files::new(temp.path());

        let tasks = vec![ParsedTask {
            id: "task_one".to_string(),
            status: "todo".to_string(),
            claimed_by: None,
            name: "First task".to_string(),
        }];

        files.update_tasks_md(&tasks).unwrap();

        let parsed = files.parse_tasks_md().unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "task_one");
    }

    #[test]
    fn test_ensure_task_details() {
        let temp = TempDir::new().unwrap();
        let files = Files::new(temp.path());
        fs::create_dir_all(files.tasks_dir()).unwrap();

        let content = r#"# Tasks

| ID | Status | Worker | Name |
|----|--------|--------|------|
| task_a | TODO | | Task A |
| task_b | DOING | worker | Task B |
"#;
        fs::write(files.tasks_md(), content).unwrap();

        files.ensure_task_details().unwrap();

        assert!(files.task_detail("task_a").exists());
        assert!(files.task_detail("task_b").exists());
    }

    #[test]
    fn test_list_chat_threads() {
        let temp = TempDir::new().unwrap();
        let files = Files::new(temp.path());
        fs::create_dir_all(files.chats_dir()).unwrap();

        fs::write(files.chat_file("user"), "# User chat").unwrap();
        fs::write(files.chat_file("group"), "# Group chat").unwrap();
        fs::write(files.chat_file("alpha"), "# Alpha chat").unwrap();

        let threads = files.list_chat_threads().unwrap();
        assert_eq!(threads, vec!["alpha", "group", "user"]);
    }

    #[test]
    fn test_list_worker_logs() {
        let temp = TempDir::new().unwrap();
        let files = Files::new(temp.path());
        let tmp_dir = temp.path().join("tmp");
        fs::create_dir_all(&tmp_dir).unwrap();

        fs::write(files.worker_log("alpha"), "log content").unwrap();
        fs::write(files.worker_log("beta"), "log content").unwrap();
        fs::write(tmp_dir.join("other.txt"), "not a log").unwrap();

        let logs = files.list_worker_logs().unwrap();
        assert_eq!(logs, vec!["alpha", "beta"]);
    }
}
