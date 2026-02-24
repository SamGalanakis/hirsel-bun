//! CLI `hirsel msg` command - send and view messages
//!
//! Provides:
//! - View messages in a thread: `hirsel msg <run_name>`
//! - Send message to thread: `hirsel msg <run_name> "message"`
//! - List available threads: `hirsel msg <run_name> --list-threads`
//!
//! Messages are stored in project-level storage (Sheepfold) so workers can see them.

use crate::cli::helpers::block_on;
use crate::cli::MsgArgs;
use crate::core::names::slugify;
use crate::core::state::{SQLiteState, StateError, WorkerUpdate};
use crate::core::ProjectMessagesStore;
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during msg operations
#[derive(Debug, Error)]
pub enum MsgError {
    #[error("State error: {0}")]
    State(#[from] StateError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Run not found: {0}")]
    RunNotFound(String),

    #[error("Run not linked to project: {0}")]
    NotLinkedToProject(String),

    #[error("Project messages error: {0}")]
    ProjectMessages(String),
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

    let state = block_on(SQLiteState::new(&run_name))?;

    // Get project_id - required for messaging
    let project_id = block_on(state.get_project_id())?
        .ok_or_else(|| MsgError::NotLinkedToProject(run_name.clone()))?;

    // Get route_id (defaults to main route)
    let route_id = block_on(state.get_route_id()).unwrap_or(1);

    let store = block_on(ProjectMessagesStore::open())
        .map_err(|e| MsgError::ProjectMessages(e.to_string()))?;

    // Handle --list-threads
    if args.list_threads {
        let thread_summaries = block_on(store.get_threads(project_id, route_id, "user"))
            .map_err(|e| MsgError::ProjectMessages(e.to_string()))?;

        let threads = thread_summaries
            .into_iter()
            .map(|t| ThreadInfo {
                name: t.thread,
                message_count: t.message_count,
            })
            .collect();

        return Ok(MsgOutput::ThreadList { run_name, threads });
    }

    // View messages if no message provided
    if args.message.is_none() {
        let messages = block_on(store.get_messages(project_id, route_id, &args.thread, Some(100)))
            .map_err(|e| MsgError::ProjectMessages(e.to_string()))?;

        let message_views = messages
            .into_iter()
            .map(|m| MessageView {
                sender: m.sender,
                content: m.content,
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

    block_on(store.add_message(
        project_id,
        route_id,
        &args.thread,
        "user",
        message_text,
        false,
    ))
    .map_err(|e| MsgError::ProjectMessages(e.to_string()))?;

    // Check for waiting workers on this thread and resume them
    let resumed_workers = resume_waiting_workers(&state, &args.thread)?;

    Ok(MsgOutput::Sent {
        thread: args.thread.clone(),
        resumed_workers,
    })
}

/// Resume workers that are waiting on a thread
fn resume_waiting_workers(state: &SQLiteState, thread: &str) -> MsgResult<Vec<String>> {
    let workers = block_on(state.get_workers())?;
    let mut resumed = Vec::new();

    for worker in workers {
        if worker.hitl_waiting && worker.waiting_thread.as_deref() == Some(thread) {
            // Clear waiting thread and hitl_waiting flag
            block_on(state.update_worker(
                &worker.name,
                WorkerUpdate {
                    waiting_thread: Some(String::new()), // Clear waiting thread
                    hitl_waiting: Some(false),
                    ..Default::default()
                },
            ))?;
            resumed.push(worker.name.clone());
        }
    }

    Ok(resumed)
}

/// Get available threads for a run
pub fn get_available_threads(run_name: &str) -> MsgResult<Vec<String>> {
    let run_name = slugify(run_name);
    let runs_dir = get_runs_dir();
    let run_dir = runs_dir.join(&run_name);

    if !run_dir.exists() {
        return Err(MsgError::RunNotFound(run_name));
    }

    let state = block_on(SQLiteState::new(&run_name))?;

    let project_id = block_on(state.get_project_id())?
        .ok_or_else(|| MsgError::NotLinkedToProject(run_name.clone()))?;

    let route_id = block_on(state.get_route_id()).unwrap_or(1);

    let store = block_on(ProjectMessagesStore::open())
        .map_err(|e| MsgError::ProjectMessages(e.to_string()))?;

    let threads = block_on(store.get_threads(project_id, route_id, "user"))
        .map_err(|e| MsgError::ProjectMessages(e.to_string()))?;

    Ok(threads.into_iter().map(|t| t.thread).collect())
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
