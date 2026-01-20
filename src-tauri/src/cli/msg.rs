//! CLI `hirsel msg` command - send and view messages
//!
//! Provides:
//! - View messages in a thread: `hirsel msg <run_name>`
//! - Send message to thread: `hirsel msg <run_name> "message"`
//! - List available threads: `hirsel msg <run_name> --list-threads`

use crate::cli::MsgArgs;
use crate::core::chats::{append_message_to_file, get_thread_names, ChatError, Message};
use crate::core::names::slugify;
use crate::core::state::{SQLiteState, StateError, WorkerUpdate};
use crate::core::Files;
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during msg operations
#[derive(Debug, Error)]
pub enum MsgError {
    #[error("State error: {0}")]
    State(#[from] StateError),

    #[error("Chat error: {0}")]
    Chat(#[from] ChatError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Run not found: {0}")]
    RunNotFound(String),

    #[error("Chats directory not found for run: {0}")]
    ChatsNotFound(String),

    #[error("Thread not found: {0}")]
    ThreadNotFound(String),
}

pub type MsgResult<T> = Result<T, MsgError>;

/// Output from viewing messages
#[derive(Debug, serde::Serialize)]
pub struct MessageView {
    pub sender: String,
    pub content: String,
    pub timestamp: String,
    pub waiting: bool,
}

/// Output from the msg command
#[derive(Debug, serde::Serialize)]
pub enum MsgOutput {
    /// List of available threads with message counts
    ThreadList {
        run_name: String,
        threads: Vec<ThreadInfo>,
    },
    /// Messages in a thread
    Messages {
        run_name: String,
        thread: String,
        messages: Vec<MessageView>,
    },
    /// Message was sent successfully
    Sent {
        thread: String,
        resumed_workers: Vec<String>,
    },
}

/// Information about a thread
#[derive(Debug, serde::Serialize)]
pub struct ThreadInfo {
    pub name: String,
    pub message_count: i64,
}

/// Get runs directory - defaults to ~/.hirsel/runs
fn get_runs_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hirsel")
        .join("runs")
}

/// Run the msg command
pub fn run(args: &MsgArgs) -> MsgResult<MsgOutput> {
    let run_name = slugify(&args.run_name);
    let runs_dir = get_runs_dir();
    let run_dir = runs_dir.join(&run_name);

    // Check run exists
    if !run_dir.exists() {
        return Err(MsgError::RunNotFound(run_name));
    }

    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path())?;

    // Check chats directory exists
    let chats_dir = files.chats_dir();
    if !chats_dir.exists() {
        return Err(MsgError::ChatsNotFound(run_name));
    }

    // Handle --list-threads
    if args.list_threads {
        let thread_names = get_thread_names(&chats_dir)?;
        let threads: Vec<ThreadInfo> = thread_names
            .into_iter()
            .map(|name| {
                let count = state.get_thread_message_count(&name).unwrap_or(0);
                ThreadInfo {
                    name,
                    message_count: count,
                }
            })
            .collect();

        return Ok(MsgOutput::ThreadList { run_name, threads });
    }

    // Check thread exists
    let chat_path = files.chat_file(&args.thread);
    if !chat_path.exists() {
        return Err(MsgError::ThreadNotFound(args.thread.clone()));
    }

    // View messages if no message provided
    if args.message.is_none() {
        let messages = state.get_messages(&args.thread, 100)?;
        let message_views: Vec<MessageView> = messages
            .into_iter()
            .map(|m| MessageView {
                sender: m.sender,
                content: m.content,
                // timestamp is already a String from state
                timestamp: if m.timestamp.len() > 16 {
                    m.timestamp[..16].to_string()
                } else {
                    m.timestamp
                },
                waiting: m.waiting,
            })
            .collect();

        return Ok(MsgOutput::Messages {
            run_name,
            thread: args.thread.clone(),
            messages: message_views,
        });
    }

    // Send message
    let message_text = args.message.as_ref().unwrap();

    // Write to both SQLite and file
    state.add_message(&args.thread, "user", message_text, false)?;

    let message = Message::new("user", message_text.as_str());
    append_message_to_file(&chat_path, &message)?;

    // Check for waiting workers on this thread and resume them
    let resumed_workers = resume_waiting_workers(&state, &args.thread)?;

    Ok(MsgOutput::Sent {
        thread: args.thread.clone(),
        resumed_workers,
    })
}

/// Resume workers that are waiting on a thread
fn resume_waiting_workers(state: &SQLiteState, thread: &str) -> MsgResult<Vec<String>> {
    let workers = state.get_workers()?;
    let mut resumed = Vec::new();

    for worker in workers {
        if worker.hitl_waiting && worker.waiting_thread.as_deref() == Some(thread) {
            // Clear waiting thread and hitl_waiting flag
            state.update_worker(
                &worker.name,
                WorkerUpdate {
                    waiting_thread: Some(String::new()), // Clear waiting thread
                    hitl_waiting: Some(false),
                    ..Default::default()
                },
            )?;
            resumed.push(worker.name.clone());

            // Note: Actual worker resumption (spawning ACP client) would happen here
            // in a full implementation. For now we just record which workers would be resumed.
        }
    }

    Ok(resumed)
}

/// Get available threads for error messages
pub fn get_available_threads(run_name: &str) -> MsgResult<Vec<String>> {
    let run_name = slugify(run_name);
    let runs_dir = get_runs_dir();
    let run_dir = runs_dir.join(&run_name);

    if !run_dir.exists() {
        return Err(MsgError::RunNotFound(run_name));
    }

    let files = Files::new(&run_dir);
    let chats_dir = files.chats_dir();

    if !chats_dir.exists() {
        return Ok(Vec::new());
    }

    Ok(get_thread_names(&chats_dir)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_runs_dir() {
        let runs_dir = get_runs_dir();
        assert!(runs_dir.to_string_lossy().contains(".hirsel"));
        assert!(runs_dir.to_string_lossy().ends_with("runs"));
    }

    #[test]
    fn test_run_not_found() {
        let args = MsgArgs {
            run_name: "nonexistent-run-xyz".to_string(),
            message: None,
            thread: "user".to_string(),
            list_threads: false,
        };

        let result = run(&args);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MsgError::RunNotFound(_)));
    }

    #[test]
    fn test_thread_info() {
        let info = ThreadInfo {
            name: "user".to_string(),
            message_count: 5,
        };
        assert_eq!(info.name, "user");
        assert_eq!(info.message_count, 5);
    }

    #[test]
    fn test_message_view() {
        let view = MessageView {
            sender: "alice".to_string(),
            content: "Hello".to_string(),
            timestamp: "2024-01-15 10:30".to_string(),
            waiting: false,
        };
        assert_eq!(view.sender, "alice");
        assert!(!view.waiting);
    }
}
