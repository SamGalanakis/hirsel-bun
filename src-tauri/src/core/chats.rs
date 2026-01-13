//! Chat/messaging system for hirsel
//!
//! Handles:
//! - Markdown-based chat files (user.md, group.md, learnings.md, worker.md)
//! - Message formatting with timestamps
//! - Thread management and creation

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Chat mode - determines read/write permissions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChatMode {
    /// Both parties can read and write
    TwoWay,
    /// Only one party can write, others read-only
    ReadOnly,
}

impl ChatMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChatMode::TwoWay => "two-way",
            ChatMode::ReadOnly => "read-only",
        }
    }
}

/// Header information for a chat thread
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatHeader {
    pub name: String,
    pub mode: ChatMode,
    pub run: Option<String>,
    pub description: String,
}

impl ChatHeader {
    pub fn new(name: impl Into<String>, mode: ChatMode) -> Self {
        Self {
            name: name.into(),
            mode,
            run: None,
            description: String::new(),
        }
    }

    pub fn with_run(mut self, run: impl Into<String>) -> Self {
        self.run = Some(run.into());
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }
}

/// A message in a chat thread
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub sender: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub waiting: bool,
}

impl Message {
    pub fn new(sender: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            sender: sender.into(),
            content: content.into(),
            timestamp: Utc::now(),
            waiting: false,
        }
    }

    pub fn with_waiting(mut self, waiting: bool) -> Self {
        self.waiting = waiting;
        self
    }

    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }
}

/// Errors that can occur in chat operations
#[derive(Debug, Error)]
pub enum ChatError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Chat directory does not exist: {0}")]
    DirectoryNotFound(PathBuf),

    #[error("Chat file not found: {0}")]
    FileNotFound(PathBuf),
}

pub type Result<T> = std::result::Result<T, ChatError>;

/// Format a chat header as markdown
pub fn format_chat_header(header: &ChatHeader) -> String {
    let mut lines = vec![format!("# {}", header.name), String::new()];

    lines.push(format!("| Mode | {} |", header.mode.as_str()));
    if let Some(ref run) = header.run {
        lines.push(format!("| Run  | {} |", run));
    }
    lines.push(String::new());

    if !header.description.is_empty() {
        lines.push(header.description.clone());
        lines.push(String::new());
    }

    lines.push("---".to_string());
    lines.push(String::new());

    lines.join("\n")
}

/// Format a message as markdown
pub fn format_message(message: &Message) -> String {
    let time_str = message.timestamp.format("%Y-%m-%d %H:%M").to_string();
    let waiting_marker = if message.waiting { " [WAITING]" } else { "" };

    let lines = vec![
        format!("### {} \u{00b7} {}{}", message.sender, time_str, waiting_marker),
        String::new(),
        message.content.clone(),
        String::new(),
        "---".to_string(),
        String::new(),
    ];

    lines.join("\n")
}

/// Append a message to a chat file
pub fn append_message_to_file(path: &Path, message: &Message) -> Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;

    write!(file, "{}", format_message(message))?;
    Ok(())
}

/// Create the default user chat thread
pub fn create_default_user_chat(chats_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(chats_dir)?;
    let user_chat = chats_dir.join("user.md");

    if user_chat.exists() {
        return Ok(user_chat);
    }

    let header = ChatHeader::new("user", ChatMode::TwoWay).with_description(
        "Message the user if you are stuck, need clarification, or require human assistance.\n\
         Do not message for trivial updates - only when genuinely blocked.",
    );

    fs::write(&user_chat, format_chat_header(&header))?;
    Ok(user_chat)
}

/// Create the default group chat thread
pub fn create_default_group_chat(
    chats_dir: &Path,
    worker_names: &[String],
    leader: Option<&str>,
) -> Result<PathBuf> {
    fs::create_dir_all(chats_dir)?;
    let group_chat = chats_dir.join("group.md");

    if group_chat.exists() {
        return Ok(group_chat);
    }

    let workers_list = worker_names.join(", ");
    let leader_line = leader
        .map(|l| format!("\nLeader: {} (responsible for initial task breakdown).", l))
        .unwrap_or_default();

    let description = format!(
        "Coordination channel for workers: {}.{}\n\
         Use this to share discoveries, coordinate work, or ask other workers questions.\n\
         All workers can read and write.",
        workers_list, leader_line
    );

    let header = ChatHeader::new("group", ChatMode::TwoWay).with_description(description);

    fs::write(&group_chat, format_chat_header(&header))?;
    Ok(group_chat)
}

/// Create the learnings thread for shared knowledge
pub fn create_learnings_thread(chats_dir: &Path, worker_names: &[String]) -> Result<PathBuf> {
    fs::create_dir_all(chats_dir)?;
    let learnings_chat = chats_dir.join("learnings.md");

    if learnings_chat.exists() {
        return Ok(learnings_chat);
    }

    let workers_list = worker_names.join(", ");
    let description = format!(
        "Shared knowledge base for workers: {}.\n\n\
         **Purpose:** Record discoveries, patterns, gotchas, and insights.\n\
         **Read this** at the start of each task to benefit from past learnings.\n\
         **Write here** when you discover something useful for future work.\n\n\
         Keep entries concise and actionable.",
        workers_list
    );

    let header = ChatHeader::new("learnings", ChatMode::TwoWay).with_description(description);

    fs::write(&learnings_chat, format_chat_header(&header))?;
    Ok(learnings_chat)
}

/// Create a direct message thread for a worker
pub fn create_worker_chat(chats_dir: &Path, worker_name: &str) -> Result<PathBuf> {
    fs::create_dir_all(chats_dir)?;
    let chat_path = chats_dir.join(format!("{}.md", worker_name));

    if chat_path.exists() {
        return Ok(chat_path);
    }

    let header = ChatHeader::new(worker_name, ChatMode::TwoWay)
        .with_description(format!("Direct messages with worker {}.", worker_name));

    fs::write(&chat_path, format_chat_header(&header))?;
    Ok(chat_path)
}

/// Get all thread names in a chats directory
pub fn get_thread_names(chats_dir: &Path) -> Result<Vec<String>> {
    if !chats_dir.exists() {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    for entry in fs::read_dir(chats_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "md").unwrap_or(false) {
            if let Some(stem) = path.file_stem() {
                names.push(stem.to_string_lossy().to_string());
            }
        }
    }

    names.sort();
    Ok(names)
}

/// Rewrite a chat file with new messages while preserving the header
pub fn rewrite_chat_file(path: &Path, messages: &[Message]) -> Result<()> {
    if !path.exists() {
        return Err(ChatError::FileNotFound(path.to_path_buf()));
    }

    let content = fs::read_to_string(path)?;

    // Find the end of the header (after first ---)
    let header = match content.find("---\n") {
        Some(idx) => &content[..idx + 4],
        None => &content[..],
    };

    // Build new content with header + messages
    let mut new_content = String::from(header);
    new_content.push('\n');

    for msg in messages {
        new_content.push_str(&format_message(msg));
    }

    fs::write(path, new_content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_format_chat_header() {
        let header = ChatHeader::new("test", ChatMode::TwoWay)
            .with_description("Test description");

        let formatted = format_chat_header(&header);
        assert!(formatted.contains("# test"));
        assert!(formatted.contains("| Mode | two-way |"));
        assert!(formatted.contains("Test description"));
        assert!(formatted.contains("---"));
    }

    #[test]
    fn test_format_message() {
        let msg = Message::new("alice", "Hello, world!");
        let formatted = format_message(&msg);

        assert!(formatted.contains("### alice"));
        assert!(formatted.contains("Hello, world!"));
        assert!(formatted.contains("---"));
    }

    #[test]
    fn test_format_message_waiting() {
        let msg = Message::new("bob", "Need help").with_waiting(true);
        let formatted = format_message(&msg);

        assert!(formatted.contains("[WAITING]"));
    }

    #[test]
    fn test_create_user_chat() {
        let temp_dir = TempDir::new().unwrap();
        let chats_dir = temp_dir.path().join("chats");

        let path = create_default_user_chat(&chats_dir).unwrap();
        assert!(path.exists());
        assert_eq!(path.file_name().unwrap(), "user.md");

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("# user"));
        assert!(content.contains("| Mode | two-way |"));
    }

    #[test]
    fn test_create_group_chat() {
        let temp_dir = TempDir::new().unwrap();
        let chats_dir = temp_dir.path().join("chats");

        let workers = vec!["alpha".to_string(), "beta".to_string()];
        let path = create_default_group_chat(&chats_dir, &workers, Some("alpha")).unwrap();

        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("alpha, beta"));
        assert!(content.contains("Leader: alpha"));
    }

    #[test]
    fn test_get_thread_names() {
        let temp_dir = TempDir::new().unwrap();
        let chats_dir = temp_dir.path().join("chats");
        fs::create_dir_all(&chats_dir).unwrap();

        fs::write(chats_dir.join("user.md"), "test").unwrap();
        fs::write(chats_dir.join("group.md"), "test").unwrap();
        fs::write(chats_dir.join("other.txt"), "test").unwrap(); // Should be ignored

        let names = get_thread_names(&chats_dir).unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"user".to_string()));
        assert!(names.contains(&"group".to_string()));
    }

    #[test]
    fn test_append_and_rewrite() {
        let temp_dir = TempDir::new().unwrap();
        let chats_dir = temp_dir.path().join("chats");
        let path = create_default_user_chat(&chats_dir).unwrap();

        // Append a message
        let msg1 = Message::new("alice", "First message");
        append_message_to_file(&path, &msg1).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("First message"));

        // Rewrite with different messages
        let msg2 = Message::new("bob", "Replacement message");
        rewrite_chat_file(&path, &[msg2]).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("First message"));
        assert!(content.contains("Replacement message"));
        assert!(content.contains("# user")); // Header preserved
    }
}
