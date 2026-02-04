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

/// Start or restart the daemon
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
