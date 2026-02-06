//! Session metrics calculation for hirsel.
//!
//! This module provides functions to extract metrics from Claude Code
//! session files, including turn count, token usage, and context utilization.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde::Deserialize;
use tracing::debug;

use crate::core::config::{get_context_window, AgentType, Config};
use crate::core::constants::METRICS_CACHE_TTL;

static METRICS_CACHE: OnceLock<Mutex<HashMap<String, (Instant, SessionMetrics)>>> = OnceLock::new();

fn metrics_cache() -> &'static Mutex<HashMap<String, (Instant, SessionMetrics)>> {
    METRICS_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Session metrics data
#[derive(Debug, Clone, Default)]
pub struct SessionMetrics {
    pub turns: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub model: Option<String>,
    pub context_window: Option<u32>,
    pub context_utilization: Option<f64>,
}

/// Message type from session file
#[derive(Deserialize)]
struct SessionEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    message: Option<AssistantMessage>,
}

#[derive(Deserialize)]
struct AssistantMessage {
    model: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

/// Get the path to a Claude session file
fn get_session_file(session_id: &str, project_path: &str) -> PathBuf {
    // Claude stores sessions in ~/.claude/projects/<escaped-path>/<session-id>.jsonl
    let escaped_path = project_path.replace(['/', '.'], "-");

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("projects")
        .join(escaped_path)
        .join(format!("{}.jsonl", session_id))
}

/// Get session metrics for a worker
///
/// This function reads the Claude session file and extracts metrics.
/// Results are cached for a short time to avoid repeated file reads.
pub fn get_session_metrics(session_id: Option<&str>, project_path: Option<&str>) -> SessionMetrics {
    let default = SessionMetrics::default();

    let (session_id, project_path) = match (session_id, project_path) {
        (Some(s), Some(p)) => (s, p),
        _ => return default,
    };

    // Check cache first
    let cache_key = format!("{}:{}", session_id, project_path);
    {
        let cache = metrics_cache().lock().unwrap();
        if let Some((cached_time, cached_data)) = cache.get(&cache_key) {
            if cached_time.elapsed() < METRICS_CACHE_TTL {
                return cached_data.clone();
            }
        }
    }

    // Read session file
    let session_file = get_session_file(session_id, project_path);
    if !session_file.exists() {
        return default;
    }

    let content = match std::fs::read_to_string(&session_file) {
        Ok(c) => c,
        Err(e) => {
            debug!("Could not read session file {:?}: {}", session_file, e);
            return default;
        }
    };

    let mut turns = 0u32;
    let mut total_input_tokens = 0u64;
    let mut total_output_tokens = 0u64;
    let mut last_input_tokens = 0u64;
    let mut last_output_tokens = 0u64;
    let mut model: Option<String> = None;

    for line in content.lines() {
        if line.is_empty() {
            continue;
        }

        let entry: SessionEntry = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        match entry.entry_type.as_deref() {
            Some("user") => {
                turns += 1;
            }
            Some("assistant") => {
                if let Some(msg) = entry.message {
                    if let Some(usage) = msg.usage {
                        // Sum input tokens including cache tokens
                        let msg_input = usage.input_tokens.unwrap_or(0)
                            + usage.cache_creation_input_tokens.unwrap_or(0)
                            + usage.cache_read_input_tokens.unwrap_or(0);
                        let msg_output = usage.output_tokens.unwrap_or(0);

                        total_input_tokens += msg_input;
                        total_output_tokens += msg_output;

                        // Track last message's tokens for context utilization
                        last_input_tokens = msg_input;
                        last_output_tokens = msg_output;
                    }

                    // Track model (use last seen)
                    if let Some(m) = msg.model {
                        model = Some(m);
                    }
                }
            }
            _ => {}
        }
    }

    // Calculate context utilization for Claude models
    let config = Config::default();
    let (context_window, context_utilization) = if let Some(model_name) = model.as_ref() {
        if config.agent.agent_type() != AgentType::Claude {
            (None, None)
        } else {
            let window = get_context_window(model_name);

            // Current context size is approximately last input + output
            let current_context = last_input_tokens + last_output_tokens;
            let utilization = if window > 0 {
                Some(current_context as f64 / window as f64)
            } else {
                None
            };

            (Some(window), utilization)
        }
    } else {
        (None, None)
    };

    let metrics = SessionMetrics {
        turns,
        input_tokens: total_input_tokens,
        output_tokens: total_output_tokens,
        model,
        context_window,
        context_utilization,
    };

    // Cache the result
    {
        let mut cache = metrics_cache().lock().unwrap();
        cache.insert(cache_key, (Instant::now(), metrics.clone()));
    }

    metrics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_session_file() {
        let path = get_session_file("abc123", "/home/user/project");
        assert!(path.to_string_lossy().contains(".claude/projects"));
        assert!(path.to_string_lossy().contains("-home-user-project"));
        assert!(path.to_string_lossy().contains("abc123.jsonl"));
    }

    #[test]
    fn test_default_metrics() {
        let metrics = get_session_metrics(None, None);
        assert_eq!(metrics.turns, 0);
        assert_eq!(metrics.input_tokens, 0);
        assert_eq!(metrics.output_tokens, 0);
        assert!(metrics.model.is_none());
    }
}
