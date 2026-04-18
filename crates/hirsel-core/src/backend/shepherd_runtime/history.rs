use base64::Engine;
use lash::{Message, MessageRole, Part, PartKind, PruneState};

use super::types::ShepherdMessageChunk;
use super::types::ShepherdScope;
use crate::backend::app::ResultExt;
use crate::backend::ShepherdChatMessage;
use crate::backend::{ShepherdChatMessageOptions, ShepherdChatStore, ShepherdLiveTurn};

pub(super) const MAX_IMAGE_COUNT: usize = 8;
pub(super) const MAX_IMAGE_BASE64_CHARS: usize = 12 * 1024 * 1024;
pub(super) const RUNTIME_HISTORY_LIMIT: usize = 48;
const RUNTIME_PREVIEW_MAX_CHARS: usize = 1200;

pub(super) fn validate_chunks(chunks: &[ShepherdMessageChunk]) -> Result<(), String> {
    if chunks.is_empty() {
        return Err("message must contain at least one chunk".to_string());
    }

    let mut image_count = 0usize;
    for chunk in chunks {
        match chunk {
            ShepherdMessageChunk::Text { content } | ShepherdMessageChunk::Thinking { content } => {
                if content.trim().is_empty() {
                    return Err("text/thinking chunk content cannot be empty".to_string());
                }
            }
            ShepherdMessageChunk::Notice { tone, content, .. } => {
                if tone.trim().is_empty() || content.trim().is_empty() {
                    return Err("notice chunk requires non-empty tone/content".to_string());
                }
            }
            ShepherdMessageChunk::Tool {
                id, title, status, ..
            } => {
                if id.trim().is_empty() || title.trim().is_empty() || status.trim().is_empty() {
                    return Err("tool chunk requires non-empty id/title/status".to_string());
                }
            }
            ShepherdMessageChunk::Image {
                mime_type,
                data_base64,
                ..
            } => {
                image_count += 1;
                if !mime_type.starts_with("image/") {
                    return Err(format!("invalid image mime type: {}", mime_type));
                }
                if data_base64.is_empty() {
                    return Err("image chunk dataBase64 cannot be empty".to_string());
                }
                if data_base64.len() > MAX_IMAGE_BASE64_CHARS {
                    return Err("image too large for Shepherd message".to_string());
                }
            }
            ShepherdMessageChunk::Skill { name, path, .. } => {
                if name.trim().is_empty() || path.trim().is_empty() {
                    return Err("skill chunk requires non-empty name/path".to_string());
                }
            }
            ShepherdMessageChunk::FileRef { root_id, path, .. } => {
                if root_id.trim().is_empty() || path.trim().is_empty() {
                    return Err("file ref chunk requires non-empty rootId/path".to_string());
                }
            }
        }
    }

    if image_count > MAX_IMAGE_COUNT {
        return Err(format!(
            "too many images in one message (max {})",
            MAX_IMAGE_COUNT
        ));
    }

    Ok(())
}

pub(super) fn chunks_to_json(chunks: &[ShepherdMessageChunk]) -> Result<String, String> {
    validate_chunks(chunks)?;
    serde_json::to_string(chunks).map_err(|e| format!("failed to serialize chunks: {}", e))
}

pub(super) fn build_user_chunks(
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
) -> Result<Vec<ShepherdMessageChunk>, String> {
    if let Some(chunks) = chunks {
        validate_chunks(&chunks)?;
        return Ok(chunks);
    }

    let text = content.unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return Err("message content is empty".to_string());
    }

    Ok(vec![ShepherdMessageChunk::Text { content: text }])
}

pub(super) fn chunk_text(chunks: &[ShepherdMessageChunk]) -> String {
    chunks
        .iter()
        .filter_map(|chunk| match chunk {
            ShepherdMessageChunk::Text { content } => Some(content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn chunk_image_count(chunks: &[ShepherdMessageChunk]) -> usize {
    chunks
        .iter()
        .filter(|c| matches!(c, ShepherdMessageChunk::Image { .. }))
        .count()
}

pub(super) fn decode_png_images(chunks: &[ShepherdMessageChunk]) -> Result<Vec<Vec<u8>>, String> {
    let mut images = Vec::new();

    for chunk in chunks {
        let ShepherdMessageChunk::Image {
            mime_type,
            data_base64,
            ..
        } = chunk
        else {
            continue;
        };

        if mime_type != "image/png" {
            return Err(format!(
                "Shepherd currently supports pasted PNG images only (got {})",
                mime_type
            ));
        }

        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data_base64)
            .map_err(|e| format!("invalid image dataBase64: {}", e))?;
        images.push(decoded);
    }

    Ok(images)
}

pub(super) fn parse_chunks_from_json(chunks_json: &str) -> Vec<ShepherdMessageChunk> {
    serde_json::from_str(chunks_json).unwrap_or_default()
}

fn truncate_for_runtime(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let count = trimmed.chars().count();
    if count <= max_chars {
        return trimmed.to_string();
    }

    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

fn summarize_chunks_for_runtime(chunks: &[ShepherdMessageChunk]) -> String {
    let mut text_parts: Vec<String> = Vec::new();
    let mut image_count = 0usize;
    let mut tool_notes: Vec<String> = Vec::new();

    for chunk in chunks {
        match chunk {
            ShepherdMessageChunk::Text { content } => {
                let t = content.trim();
                if !t.is_empty() {
                    text_parts.push(t.to_string());
                }
            }
            ShepherdMessageChunk::Notice { .. } => {}
            ShepherdMessageChunk::Tool { title, status, .. } => {
                tool_notes.push(format!("{}({})", title, status));
            }
            ShepherdMessageChunk::Image { .. } => {
                image_count += 1;
            }
            ShepherdMessageChunk::Skill { name, .. } => {
                tool_notes.push(format!("skill({})", name));
            }
            ShepherdMessageChunk::FileRef {
                root_id,
                path,
                line_start,
                line_end,
            } => {
                let suffix = match (line_start, line_end) {
                    (Some(start), Some(end)) if start == end => format!(":{}", start),
                    (Some(start), Some(end)) => format!(":{}-{}", start, end),
                    _ => String::new(),
                };
                tool_notes.push(format!("file({}:{}{})", root_id, path, suffix));
            }
            ShepherdMessageChunk::Thinking { .. } => {}
        }
    }

    let mut out = text_parts.join("\n\n");

    if image_count > 0 {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!(
            "[{} image attachment{}]",
            image_count,
            if image_count == 1 { "" } else { "s" }
        ));
    }

    if !tool_notes.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("[tool activity: ");
        out.push_str(&tool_notes.join(", "));
        out.push(']');
    }

    truncate_for_runtime(&out, RUNTIME_PREVIEW_MAX_CHARS)
}

fn history_role_to_message_role(role: &str) -> Option<MessageRole> {
    match role {
        "user" => Some(MessageRole::User),
        "assistant" => Some(MessageRole::Assistant),
        "system" => Some(MessageRole::System),
        _ => None,
    }
}

pub(super) fn build_runtime_messages(history: &[ShepherdChatMessage]) -> Vec<Message> {
    let mut messages = Vec::with_capacity(history.len());

    for item in history {
        let Some(role) = history_role_to_message_role(item.role.as_str()) else {
            continue;
        };

        let summary = summarize_chunks_for_runtime(&parse_chunks_from_json(&item.chunks_json));
        if summary.is_empty() {
            continue;
        }

        let message_id = format!("m{}", messages.len());
        messages.push(Message {
            id: message_id.clone(),
            role,
            parts: vec![Part {
                id: format!("{}.p0", message_id),
                kind: PartKind::Text,
                content: summary,
                attachment: None,
                tool_call_id: None,
                tool_name: None,
                prune_state: PruneState::Intact,
            }],
            origin: None,
            user_input: None,
        });
    }

    messages
}

pub(super) async fn load_scope_messages(
    scope: &ShepherdScope,
    limit: usize,
) -> Result<Vec<ShepherdChatMessage>, String> {
    let store = ShepherdChatStore::open().await.str_err()?;

    let mut messages = match scope {
        ShepherdScope::General => store.get_messages(None).await.str_err()?,
        ShepherdScope::Shepherd { project_id, .. } => store
            .get_scope_messages(
                Some(*project_id),
                Some(&ShepherdChatStore::shepherd_scope_key(*project_id)),
                limit,
            )
            .await
            .str_err()?,
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => store
            .get_scope_messages(
                Some(*project_id),
                Some(&ShepherdChatStore::thread_scope_key(thread_id)),
                limit,
            )
            .await
            .str_err()?,
    };

    if !matches!(
        scope,
        ShepherdScope::Shepherd { .. } | ShepherdScope::Thread { .. }
    ) && messages.len() > limit
    {
        let start = messages.len().saturating_sub(limit);
        messages = messages.split_off(start);
    }

    Ok(messages)
}

pub(super) async fn save_message(
    scope: &ShepherdScope,
    role: &str,
    chunks_json: &str,
) -> Result<i64, String> {
    save_message_with_options(scope, role, chunks_json, &Default::default()).await
}

pub(super) async fn save_message_with_options(
    scope: &ShepherdScope,
    role: &str,
    chunks_json: &str,
    options: &ShepherdChatMessageOptions,
) -> Result<i64, String> {
    let store = ShepherdChatStore::open().await.str_err()?;
    match scope {
        ShepherdScope::General => store
            .save_message_with_options(None, role, chunks_json, options)
            .await
            .str_err(),
        ShepherdScope::Shepherd { project_id, .. } => store
            .save_scope_message_with_options(
                Some(*project_id),
                Some(&ShepherdChatStore::shepherd_scope_key(*project_id)),
                role,
                chunks_json,
                options,
            )
            .await
            .str_err(),
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => store
            .save_scope_message_with_options(
                Some(*project_id),
                Some(&ShepherdChatStore::thread_scope_key(thread_id)),
                role,
                chunks_json,
                options,
            )
            .await
            .str_err(),
    }
}

pub(super) async fn load_scope_live_turn(
    scope: &ShepherdScope,
) -> Result<Option<ShepherdLiveTurn>, String> {
    let store = ShepherdChatStore::open().await.str_err()?;
    match scope {
        ShepherdScope::General => store.get_live_turn(None, "general").await.str_err(),
        ShepherdScope::Shepherd { project_id, .. } => store
            .get_live_turn(
                Some(*project_id),
                &ShepherdChatStore::shepherd_scope_key(*project_id),
            )
            .await
            .str_err(),
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => store
            .get_live_turn(
                Some(*project_id),
                &ShepherdChatStore::thread_scope_key(thread_id),
            )
            .await
            .str_err(),
    }
}
