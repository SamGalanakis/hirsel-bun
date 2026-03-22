//! Log-related commands
//!
//! Commands for reading eval logs, history, and eval specs/results.
//! Worker events are streamed from the database via events.rs.

use serde::{Deserialize, Serialize};

use super::ResultExt;
use crate::core::api_types::{Eval, HistoryEntry};
use crate::core::config;
use crate::core::orchestrator::create_orchestrator;

/// Response for log file content (used for eval logs)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogResponse {
    pub content: String,
    pub byte_offset: u64,
    pub file_size: u64,
    pub exists: bool,
}

/// Get eval log file content
#[tracing::instrument]
#[tauri::command]
pub async fn get_eval_log(
    run_name: String,
    lines: Option<u32>,
    from_offset: Option<u64>,
) -> Result<LogResponse, String> {
    use std::io::{Read, Seek, SeekFrom};

    let run_dir = config::run_dir(&run_name);
    let files = crate::core::Files::new(&run_dir);
    let log_path = files.eval_log();

    if !log_path.exists() {
        return Ok(LogResponse {
            content: String::new(),
            byte_offset: 0,
            file_size: 0,
            exists: false,
        });
    }

    let metadata = std::fs::metadata(&log_path).context("Failed to read eval log metadata")?;
    let file_size = metadata.len();

    let mut file = std::fs::File::open(&log_path).context("Failed to open eval log")?;

    let _start_offset = if let Some(offset) = from_offset {
        if offset < file_size {
            file.seek(SeekFrom::Start(offset))
                .context("Failed to seek in eval log")?;
            offset
        } else {
            return Ok(LogResponse {
                content: String::new(),
                byte_offset: file_size,
                file_size,
                exists: true,
            });
        }
    } else {
        0
    };

    let mut content = String::new();
    file.read_to_string(&mut content)
        .context("Failed to read eval log")?;

    if let (Some(limit), None) = (lines, from_offset) {
        let limit = limit as usize;
        let all_lines: Vec<&str> = content.lines().collect();
        if all_lines.len() > limit {
            content = all_lines[all_lines.len() - limit..].join("\n");
        }
    }

    Ok(LogResponse {
        content,
        byte_offset: file_size,
        file_size,
        exists: true,
    })
}

/// Get a specific eval's log file content by path
#[tracing::instrument]
#[tauri::command]
pub async fn get_eval_log_by_path(
    run_name: String,
    log_file: String,
) -> Result<LogResponse, String> {
    use std::io::Read;
    use std::path::PathBuf;

    let run_dir = config::run_dir(&run_name);

    // Security: ensure the log file is within the run directory
    let log_path = PathBuf::from(&log_file);
    let canonical_run_dir = run_dir
        .canonicalize()
        .context("Failed to resolve run directory")?;

    // If log_file is a relative path, resolve it relative to run_dir
    let resolved_log_path = if log_path.is_relative() {
        run_dir.join(&log_path)
    } else {
        log_path.clone()
    };

    // Verify the resolved path is within the run directory
    let canonical_log_path = resolved_log_path
        .canonicalize()
        .map_err(|e| format!("Log file not found: {}", e))?;

    if !canonical_log_path.starts_with(&canonical_run_dir) {
        return Err("Log file must be within the run directory".to_string());
    }

    if !canonical_log_path.exists() {
        return Ok(LogResponse {
            content: String::new(),
            byte_offset: 0,
            file_size: 0,
            exists: false,
        });
    }

    let metadata = std::fs::metadata(&canonical_log_path).context("Failed to read log metadata")?;
    let file_size = metadata.len();

    let mut file = std::fs::File::open(&canonical_log_path).context("Failed to open log file")?;

    let mut content = String::new();
    file.read_to_string(&mut content)
        .context("Failed to read log file")?;

    Ok(LogResponse {
        content,
        byte_offset: file_size,
        file_size,
        exists: true,
    })
}

/// Get history entries for a run
/// Uses the orchestrator to support both local and remote modes
#[tracing::instrument]
#[tauri::command]
pub async fn get_history(
    run_name: String,
    limit: Option<u32>,
) -> Result<Vec<HistoryEntry>, String> {
    let orch = create_orchestrator().str_err()?;
    orch.get_history(&run_name, limit).await.str_err()
}

/// Get the eval spec (eval.md) content for a run
#[tracing::instrument]
#[tauri::command]
pub async fn get_eval_spec(run_name: String) -> Result<Option<String>, String> {
    let run_dir = config::run_dir(&run_name);
    let eval_spec_path = run_dir.join("eval.md");

    if !eval_spec_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&eval_spec_path).context("Failed to read eval spec")?;

    Ok(Some(content))
}

/// Get evals for a run
/// Uses the orchestrator to support both local and remote modes
#[tracing::instrument]
#[tauri::command]
pub async fn get_evals(run_name: String) -> Result<Vec<Eval>, String> {
    let orch = create_orchestrator().str_err()?;
    orch.list_evals(&run_name).await.str_err()
}
