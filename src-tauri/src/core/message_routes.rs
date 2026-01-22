//! Shared message HTTP route handlers.
//!
//! These functions implement the core logic for message API endpoints,
//! used by both daemon (multi-run) and coordinator_api (single-run).
//!
//! Each function takes:
//! - `&SQLiteState` for state access
//!
//! The HTTP layer (daemon/coordinator_api) is responsible for:
//! - Extracting state from headers or shared state
//! - Converting results to HTTP responses

use serde::{Deserialize, Serialize};

use crate::core::state::{Message, SQLiteState, StateResult};

// =============================================================================
// Response Types
// =============================================================================

#[derive(Serialize)]
pub struct ThreadsResponse {
    pub threads: Vec<String>,
}

#[derive(Serialize)]
pub struct ThreadSummary {
    pub name: String,
    pub message_count: i64,
}

#[derive(Serialize)]
pub struct ThreadSummariesResponse {
    pub threads: Vec<ThreadSummary>,
}

#[derive(Serialize)]
pub struct MessagesResponse {
    pub messages: Vec<Message>,
}

#[derive(Serialize)]
pub struct MessageIdResponse {
    pub id: i64,
}

#[derive(Serialize)]
pub struct UnreadCountResponse {
    pub count: i64,
}

// =============================================================================
// Request Types
// =============================================================================

#[derive(Deserialize)]
pub struct CreateMessageRequest {
    pub thread: String,
    pub sender: String,
    pub content: String,
    #[serde(default)]
    pub waiting: bool,
}

#[derive(Deserialize)]
pub struct MarkReadRequest {
    pub reader: String,
    pub up_to_id: Option<i64>,
}

// =============================================================================
// Handler Functions
// =============================================================================

/// List all thread names
pub fn list_threads(state: &SQLiteState) -> StateResult<Vec<String>> {
    state.get_threads()
}

/// List threads with message counts
pub fn list_threads_with_counts(state: &SQLiteState) -> StateResult<Vec<ThreadSummary>> {
    let threads = state.get_threads()?;
    let mut result = Vec::with_capacity(threads.len());
    for thread in threads {
        let count = state.get_thread_message_count(&thread)?;
        result.push(ThreadSummary {
            name: thread,
            message_count: count,
        });
    }
    Ok(result)
}

/// Create a new message
pub fn create_message(
    state: &SQLiteState,
    thread: &str,
    sender: &str,
    content: &str,
    waiting: bool,
) -> StateResult<i64> {
    state.add_message(thread, sender, content, waiting)
}

/// Get messages from a thread
pub fn get_messages(state: &SQLiteState, thread: &str, limit: i64) -> StateResult<Vec<Message>> {
    state.get_messages(thread, limit)
}

/// Get unread messages for a reader in a thread
pub fn get_unread_messages(
    state: &SQLiteState,
    thread: &str,
    reader: &str,
) -> StateResult<Vec<Message>> {
    state.get_unread_messages(thread, reader)
}

/// Get all unread messages for a reader across all threads
pub fn get_all_unread(state: &SQLiteState, reader: &str) -> StateResult<Vec<Message>> {
    state.get_all_unread_messages(reader)
}

/// Mark messages as read
pub fn mark_messages_read(
    state: &SQLiteState,
    thread: &str,
    reader: &str,
    up_to_id: Option<i64>,
) -> StateResult<()> {
    state.mark_messages_read(thread, reader, up_to_id)
}

/// Get unread count for notifications
pub fn get_unread_count(state: &SQLiteState) -> StateResult<i64> {
    state.get_unread_count()
}

/// Clear unread count
pub fn clear_unread(state: &SQLiteState) -> StateResult<()> {
    state.clear_unread()
}

/// Increment unread count
pub fn increment_unread(state: &SQLiteState) -> StateResult<()> {
    state.increment_unread()
}

/// Get message count for a specific thread
pub fn get_thread_message_count(state: &SQLiteState, thread: &str) -> StateResult<i64> {
    state.get_thread_message_count(thread)
}
