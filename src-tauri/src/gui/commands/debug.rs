//! Debug and utility commands
//!
//! Commands for debugging, frontend logging, version info, and Gyp chat history management.

use crate::core::gyp_chat::{GypChatMessage, GypChatStore};
use crate::version;

/// Version information response
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub version: &'static str,
    pub git_sha: &'static str,
    pub build_date: &'static str,
    pub features: Vec<&'static str>,
    pub full_version: String,
}

/// Get version and build information
#[tauri::command]
pub fn get_version() -> VersionInfo {
    VersionInfo {
        version: version::VERSION,
        git_sha: version::GIT_SHA,
        build_date: version::BUILD_DATE,
        features: version::active_features(),
        full_version: version::full_version(),
    }
}

/// Log a message from the frontend to the backend log file
/// This allows debugging frontend issues by checking the same log file
#[tauri::command]
pub async fn log_frontend(level: String, message: String) {
    match level.as_str() {
        "ERROR" => tracing::error!("[Frontend] {}", message),
        "WARN" => tracing::warn!("[Frontend] {}", message),
        "DEBUG" => tracing::debug!("[Frontend] {}", message),
        _ => tracing::info!("[Frontend] {}", message),
    }
}

/// Count claude and acp related processes (for debug panel)
#[tauri::command]
pub async fn get_process_counts() -> Result<serde_json::Value, String> {
    #[cfg(unix)]
    {
        use std::process::Command;

        // Count claude processes
        let claude_output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E '[c]laude' | wc -l")
            .output()
            .map_err(|e| e.to_string())?;
        let claude_count: i32 = String::from_utf8_lossy(&claude_output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        // Count acp processes
        let acp_output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E '[a]cp|[c]laude-code-acp' | wc -l")
            .output()
            .map_err(|e| e.to_string())?;
        let acp_count: i32 = String::from_utf8_lossy(&acp_output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        // Count node processes
        let node_output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E '[n]ode' | wc -l")
            .output()
            .map_err(|e| e.to_string())?;
        let node_count: i32 = String::from_utf8_lossy(&node_output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        // Get detailed process list
        let detail_output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E 'claude|acp' | grep -v grep | head -20")
            .output()
            .map_err(|e| e.to_string())?;
        let details = String::from_utf8_lossy(&detail_output.stdout).to_string();

        Ok(serde_json::json!({
            "claude": claude_count,
            "acp": acp_count,
            "node": node_count,
            "details": details
        }))
    }

    #[cfg(not(unix))]
    {
        Ok(serde_json::json!({
            "claude": 0,
            "acp": 0,
            "node": 0,
            "details": "Process counting not supported on this platform"
        }))
    }
}

/// Kill orphaned claude-code-acp processes (debug panel utility)
#[tauri::command]
pub async fn kill_orphaned_acp_processes() -> Result<serde_json::Value, String> {
    #[cfg(unix)]
    {
        use std::process::Command;

        // Use pkill to kill claude-code-acp processes
        let output = Command::new("pkill")
            .arg("-f")
            .arg("claude-code-acp")
            .output()
            .map_err(|e| e.to_string())?;

        // pkill returns 0 if processes were killed, 1 if none found
        let killed = if output.status.success() {
            // Count how many we killed by checking process count before/after
            // For simplicity, just report that some were killed
            1
        } else {
            0
        };

        Ok(serde_json::json!({ "killed": killed }))
    }

    #[cfg(not(unix))]
    {
        Ok(serde_json::json!({ "killed": 0 }))
    }
}

// =============================================================================
// Gyp Chat History Commands
// =============================================================================

/// Get Gyp chat history for a run (or no-run if run_name is None)
#[tauri::command]
pub async fn get_gyp_chat_history(run_name: Option<String>) -> Result<Vec<GypChatMessage>, String> {
    let store =
        GypChatStore::open().map_err(|e| format!("Failed to open gyp chat store: {}", e))?;

    let messages = store
        .get_messages(run_name.as_deref())
        .map_err(|e| format!("Failed to get gyp chat history: {}", e))?;

    Ok(messages)
}

/// Save a Gyp chat message for a run (or no-run if run_name is None)
#[tauri::command]
pub async fn save_gyp_message(
    run_name: Option<String>,
    role: String,
    chunks_json: String,
) -> Result<i64, String> {
    let store =
        GypChatStore::open().map_err(|e| format!("Failed to open gyp chat store: {}", e))?;

    let id = store
        .save_message(run_name.as_deref(), &role, &chunks_json)
        .map_err(|e| format!("Failed to save gyp message: {}", e))?;

    Ok(id)
}

/// Clear Gyp chat history for a run (or no-run if run_name is None)
#[tauri::command]
pub async fn clear_gyp_chat_history(run_name: Option<String>) -> Result<(), String> {
    let store =
        GypChatStore::open().map_err(|e| format!("Failed to open gyp chat store: {}", e))?;

    store
        .clear_messages(run_name.as_deref())
        .map_err(|e| format!("Failed to clear gyp chat history: {}", e))?;

    Ok(())
}

// GypChatStore is available from crate::core::gyp_chat for other modules
