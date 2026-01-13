//! Chat compaction module for hirsel.
//!
//! This module handles compacting chat threads when they get too long.
//! It summarizes older messages into a single compaction message and
//! removes the old messages.

use std::path::Path;
use tracing::{info, warn};

use crate::core::config::Config;
use crate::core::files::Files;
use crate::core::state::{Message, SQLiteState, StateResult};

/// Prompt template for generating compaction summaries
const COMPACTION_PROMPT: &str = r#"You are summarizing a chat thread for an AI coding agent system.

Below are messages from a shared learnings chat where workers record discoveries, patterns, and gotchas about a codebase.

Summarize ALL the learnings into a concise bullet-point list:
- One bullet per distinct learning
- Be concise (one sentence max per bullet)
- Remove duplicates (keep the most complete version)
- Preserve specific details (file paths, env vars, patterns)
- Group related items if it improves clarity
- Order by importance/usefulness

Output ONLY the bullet points, no preamble or explanation.

Messages to summarize:
"#;

/// Check if a thread should be compacted based on message count and size
pub fn should_compact(messages: &[Message], threshold: u32, keep_count: u32) -> bool {
    let keep_count = keep_count as usize;

    if messages.len() <= keep_count {
        return false;
    }

    // Only count characters in messages that would be compacted
    let messages_to_compact = if keep_count > 0 && messages.len() > keep_count {
        &messages[..messages.len() - keep_count]
    } else {
        messages
    };

    let total_chars: usize = messages_to_compact
        .iter()
        .map(|m| m.content.len())
        .sum();

    total_chars >= threshold as usize
}

/// Format messages for inclusion in the compaction prompt
pub fn format_messages_for_summary(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|msg| format!("[{}]: {}", msg.sender, msg.content))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate the full compaction prompt from messages
pub fn get_compaction_prompt(messages: &[Message]) -> String {
    format!("{}{}", COMPACTION_PROMPT, format_messages_for_summary(messages))
}

/// Perform compaction on a thread (synchronous version)
///
/// This function checks if compaction is needed, splits messages,
/// and performs the database operations. It does NOT generate the
/// summary - that must be done externally and passed in.
pub fn compact_thread_with_summary(
    state: &SQLiteState,
    files: &Files,
    thread: &str,
    summary: &str,
    threshold: Option<u32>,
    keep_count: Option<u32>,
) -> StateResult<bool> {
    // Load config defaults if not specified
    let config = Config::default();
    let threshold = threshold.unwrap_or(config.compaction_threshold.unwrap_or(10000));
    let keep_count = keep_count.unwrap_or(config.compaction_keep_messages);

    // Get all messages in thread (use large limit)
    let messages = state.get_messages(thread, 100000)?;

    if !should_compact(&messages, threshold, keep_count) {
        return Ok(false);
    }

    info!(
        "Compacting thread '{}': {} messages, keeping {}",
        thread,
        messages.len(),
        keep_count
    );

    let keep_count = keep_count as usize;

    // Split into messages to compact (older) and keep (recent)
    let messages_to_compact = if keep_count > 0 && messages.len() > keep_count {
        &messages[..messages.len() - keep_count]
    } else {
        &messages[..]
    };

    // Build the compaction message
    let compaction_msg = format!(
        "[COMPACTED] Automated summary of {} previous messages.\n\n{}",
        messages_to_compact.len(),
        summary
    );

    // Get the IDs of messages to delete
    let ids_to_delete: Vec<i64> = messages_to_compact
        .iter()
        .map(|m| m.id)
        .collect();

    // Perform compaction in database
    state.compact_messages(thread, &ids_to_delete, &compaction_msg)?;

    // Rewrite the chat file to match
    let chat_path = files.chat_file(thread);
    if chat_path.exists() {
        // Get fresh messages from DB (now includes compaction message)
        let new_messages = state.get_messages(thread, 100000)?;
        if let Err(e) = rewrite_chat_file(&chat_path, &new_messages) {
            warn!("Failed to rewrite chat file after compaction: {}", e);
        }
    }

    info!(
        "Compacted thread '{}': {} messages → 1 summary",
        thread,
        messages_to_compact.len()
    );

    Ok(true)
}

/// Check if the learnings thread needs compaction
/// Returns the messages to compact and the prompt if compaction is needed
pub fn check_learnings_compaction(
    state: &SQLiteState,
) -> StateResult<Option<(Vec<Message>, String)>> {
    let config = Config::default();

    // Check if compaction is enabled
    let threshold = match config.compaction_threshold {
        Some(t) => t,
        None => return Ok(None),
    };
    let keep_count = config.compaction_keep_messages;

    // Get all messages in learnings thread (use large limit)
    let messages = state.get_messages("learnings", 100000)?;

    if !should_compact(&messages, threshold, keep_count) {
        return Ok(None);
    }

    let keep_count = keep_count as usize;

    // Split into messages to compact (older)
    let messages_to_compact: Vec<Message> = if keep_count > 0 && messages.len() > keep_count {
        messages[..messages.len() - keep_count].to_vec()
    } else {
        messages
    };

    // Generate the prompt
    let prompt = get_compaction_prompt(&messages_to_compact);

    Ok(Some((messages_to_compact, prompt)))
}

/// Rewrite a chat file with new messages
fn rewrite_chat_file(path: &Path, messages: &[Message]) -> std::io::Result<()> {
    use std::fs::File;
    use std::io::Write;

    let mut file = File::create(path)?;

    for msg in messages {
        writeln!(file, "## {} ({})", msg.sender, msg.timestamp)?;
        writeln!(file)?;
        writeln!(file, "{}", msg.content)?;
        writeln!(file)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_compact() {
        let messages: Vec<Message> = (0..50)
            .map(|i| Message {
                id: i,
                thread: "test".to_string(),
                sender: "worker".to_string(),
                content: "x".repeat(100),
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                waiting: false,
            })
            .collect();

        // 50 messages * 100 chars = 5000 chars
        // With threshold 10000 and keep_count 10, we have 40 messages = 4000 chars to compact
        assert!(!should_compact(&messages, 10000, 10));

        // With threshold 3000, should compact
        assert!(should_compact(&messages, 3000, 10));

        // With too few messages, should not compact
        assert!(!should_compact(&messages[..5], 100, 10));
    }

    #[test]
    fn test_format_messages_for_summary() {
        let messages = vec![
            Message {
                id: 1,
                thread: "test".to_string(),
                sender: "alice".to_string(),
                content: "Hello".to_string(),
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                waiting: false,
            },
            Message {
                id: 2,
                thread: "test".to_string(),
                sender: "bob".to_string(),
                content: "World".to_string(),
                timestamp: "2024-01-01T00:00:01Z".to_string(),
                waiting: false,
            },
        ];

        let formatted = format_messages_for_summary(&messages);
        assert!(formatted.contains("[alice]: Hello"));
        assert!(formatted.contains("[bob]: World"));
    }
}
