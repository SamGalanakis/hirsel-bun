//! File system utilities for hirsel run directories.
//!
//! This module provides path accessors and file operations for the
//! run directory structure, including spec files, tasks, chats, and logs.

use chrono::Local;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

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

    /// Read a chat file's content.
    pub fn read_chat(&self, name: &str) -> io::Result<Option<String>> {
        let path = self.chat_file(name);
        if path.exists() {
            Ok(Some(fs::read_to_string(path)?))
        } else {
            Ok(None)
        }
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
