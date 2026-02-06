//! Debug and utility commands
//!
//! Commands for debugging, frontend logging, version info, and daemon health.

use super::ResultExt;
use crate::daemon;
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

/// Count claude and acp related processes (for debug panel)
#[tracing::instrument]
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
            .str_err()?;
        let claude_count: i32 = String::from_utf8_lossy(&claude_output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        // Count acp processes
        let acp_output = Command::new("sh")
            .arg("-c")
            .arg("ps aux | grep -E '[a]cp|[c]laude-code-acp' | wc -l")
            .output()
            .str_err()?;
        let acp_count: i32 = String::from_utf8_lossy(&acp_output.stdout)
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
            .arg("ps aux | grep -E 'claude|acp' | grep -v grep | head -20")
            .output()
            .str_err()?;
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

/// Kill orphaned ACP bridge processes (debug panel utility)
#[tracing::instrument]
#[tauri::command]
pub async fn kill_orphaned_acp_processes() -> Result<serde_json::Value, String> {
    #[cfg(unix)]
    {
        use std::process::Command;

        // Use pkill to kill hirsel __acp-bridge processes
        let output = Command::new("pkill")
            .arg("-f")
            .arg("__acp-bridge")
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

/// Daemon health status response
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonHealth {
    pub running: bool,
    pub version: Option<String>,
    pub git_sha: Option<String>,
    pub build_date: Option<String>,
    pub uptime_secs: Option<u64>,
    pub active_runs: Option<usize>,
    pub pid: Option<u32>,
    pub runs_dir: Option<String>,
    pub error: Option<String>,
}

/// Get daemon health status
///
/// Checks if daemon is running and returns its version info.
/// If daemon is not running, returns running=false with error message.
#[tracing::instrument]
#[tauri::command]
pub async fn get_daemon_health() -> DaemonHealth {
    let port = daemon::get_daemon_port();
    let url = format!("http://127.0.0.1:{}/daemon/status", port);

    match reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    return DaemonHealth {
                        running: true,
                        version: data
                            .get("version")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        git_sha: data
                            .get("git_sha")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        build_date: data
                            .get("build_date")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        uptime_secs: data.get("uptime_secs").and_then(|v| v.as_u64()),
                        active_runs: data
                            .get("active_runs")
                            .and_then(|v| v.as_u64())
                            .map(|n| n as usize),
                        pid: data.get("pid").and_then(|v| v.as_u64()).map(|n| n as u32),
                        runs_dir: data
                            .get("runs_dir")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        error: None,
                    };
                }
            }
            DaemonHealth {
                running: false,
                version: None,
                git_sha: None,
                build_date: None,
                uptime_secs: None,
                active_runs: None,
                pid: None,
                runs_dir: None,
                error: Some(format!("Daemon returned status: {}", status)),
            }
        }
        Err(e) => DaemonHealth {
            running: false,
            version: None,
            git_sha: None,
            build_date: None,
            uptime_secs: None,
            active_runs: None,
            pid: None,
            runs_dir: None,
            error: Some(format!("Failed to connect to daemon: {}", e)),
        },
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
    std::fs::create_dir_all(&profiling_dir)
        .map_err(|e| format!("Failed to create profiling directory: {}", e))?;

    let path = profiling_dir.join("frontend.json");

    std::fs::write(&path, &data).map_err(|e| format!("Failed to write profiling data: {}", e))?;

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

/// Start or restart the daemon
#[tracing::instrument]
#[tauri::command]
pub async fn ensure_daemon_running() -> Result<DaemonHealth, String> {
    use crate::core::orchestrator::DaemonOrchestrator;

    // Try to connect or start the daemon
    match DaemonOrchestrator::connect_or_start() {
        Ok(_) => {
            // Wait a moment for daemon to be ready
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            Ok(get_daemon_health().await)
        }
        Err(e) => Err(format!("Failed to start daemon: {}", e)),
    }
}
