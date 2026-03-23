//! Debug and utility commands
//!
//! Commands for debugging, frontend logging, version info, and local process inspection.

use super::ResultExt;
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
#[tracing::instrument]
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

/// Batch-log multiple frontend messages in a single IPC call
#[tauri::command]
pub async fn log_frontend_batch(entries: Vec<(String, String)>) {
    for (level, message) in entries {
        match level.as_str() {
            "ERROR" => tracing::error!("[Frontend] {}", message),
            "WARN" => tracing::warn!("[Frontend] {}", message),
            "DEBUG" => tracing::debug!("[Frontend] {}", message),
            _ => tracing::info!("[Frontend] {}", message),
        }
    }
}

/// Count worker-related processes (for debug panel).
#[tracing::instrument]
#[tauri::command]
pub async fn get_process_counts() -> Result<serde_json::Value, String> {
    #[cfg(unix)]
    {
        use std::process::Command;

        // Count hirsel worker processes
        let hirsel_output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E '[h]irsel .*__worker-runtime' | wc -l")
            .output()
            .str_err()?;
        let hirsel_count: i32 = String::from_utf8_lossy(&hirsel_output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        // Count node processes
        let node_output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E '[n]ode' | wc -l")
            .output()
            .str_err()?;
        let node_count: i32 = String::from_utf8_lossy(&node_output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        // Get detailed process list
        let detail_output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E 'hirsel .*__worker-runtime|node' | grep -v grep | head -20")
            .output()
            .str_err()?;
        let details = String::from_utf8_lossy(&detail_output.stdout).to_string();

        Ok(serde_json::json!({
            "hirsel": hirsel_count,
            "node": node_count,
            "details": details
        }))
    }

    #[cfg(not(unix))]
    {
        Ok(serde_json::json!({
            "hirsel": 0,
            "node": 0,
            "details": "Process counting not supported on this platform"
        }))
    }
}

/// Kill orphaned worker helper processes (debug panel utility).
#[tracing::instrument]
#[tauri::command]
pub async fn kill_orphaned_worker_processes() -> Result<serde_json::Value, String> {
    #[cfg(unix)]
    {
        use std::process::Command;

        // Use pkill to kill stale worker helper processes.
        let output = Command::new("pkill")
            .arg("-f")
            .arg("__worker-runtime")
            .output()
            .str_err()?;

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

/// Check if profiling mode is active (HIRSEL_PROFILING=1)
#[tauri::command]
pub fn get_profiling_enabled() -> bool {
    std::env::var("HIRSEL_PROFILING").as_deref() == Ok("1")
}

/// Save frontend profiling data to the current profiling session directory.
///
/// Receives JSON metrics from the frontend and writes them to frontend.json.
#[tauri::command]
pub async fn save_profiling_data(data: String) -> Result<String, String> {
    let profiling_dir = std::env::var("HIRSEL_PROFILING_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| crate::core::hirsel_dir().join("profiling"));
    std::fs::create_dir_all(&profiling_dir).context("Failed to create profiling directory")?;

    let path = profiling_dir.join("frontend.json");

    std::fs::write(&path, &data).context("Failed to write profiling data")?;

    tracing::info!("[profiling] Saved frontend data to {}", path.display());
    Ok(path.display().to_string())
}

/// Get current process RSS in MB (for profiling memory tracking)
#[tauri::command]
pub fn get_process_memory() -> Option<f64> {
    #[cfg(unix)]
    {
        // Read from /proc/self/statm - field 1 is RSS in pages
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            if let Some(rss_pages) = statm.split_whitespace().nth(1) {
                if let Ok(pages) = rss_pages.parse::<u64>() {
                    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
                    let rss_mb = (pages * page_size) as f64 / 1024.0 / 1024.0;
                    return Some((rss_mb * 100.0).round() / 100.0);
                }
            }
        }
        None
    }
    #[cfg(not(unix))]
    {
        None
    }
}
