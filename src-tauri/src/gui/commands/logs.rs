//! Log-related commands
//!
//! Commands for reading worker logs, eval logs, history, and eval specs/results.

use serde::{Deserialize, Serialize};

use super::types::{Eval, EvalStatus, HistoryEntry, SheepConfig};
use crate::core::{config, state::SQLiteState};

/// Response for worker log content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerLogResponse {
    pub content: String,
    pub byte_offset: u64,
    pub file_size: u64,
    pub exists: bool,
}

/// Parsed log line with tool activity info
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedLogLine {
    pub text: String,
    pub is_tool_start: bool,
    pub is_tool_end: bool,
    pub tool_name: Option<String>,
}

/// Get worker log file content
///
/// Returns the log content for a specific worker. Supports optional line limit
/// and byte offset for efficient tailing/streaming.
#[tauri::command]
pub async fn get_worker_log(
    run_name: String,
    worker_name: String,
    lines: Option<u32>,
    from_offset: Option<u64>,
) -> Result<WorkerLogResponse, String> {
    use std::io::{Read, Seek, SeekFrom};

    let run_dir = config::run_dir(&run_name);
    let files = crate::core::Files::new(&run_dir);
    let log_path = files.worker_log(&worker_name);

    if !log_path.exists() {
        return Ok(WorkerLogResponse {
            content: String::new(),
            byte_offset: 0,
            file_size: 0,
            exists: false,
        });
    }

    let metadata = std::fs::metadata(&log_path)
        .map_err(|e| format!("Failed to read log file metadata: {}", e))?;
    let file_size = metadata.len();

    let mut file =
        std::fs::File::open(&log_path).map_err(|e| format!("Failed to open log file: {}", e))?;

    // If offset is provided, seek to that position
    let _start_offset = if let Some(offset) = from_offset {
        if offset < file_size {
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| format!("Failed to seek in log file: {}", e))?;
            offset
        } else {
            // Already at or past end
            return Ok(WorkerLogResponse {
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
        .map_err(|e| format!("Failed to read log file: {}", e))?;

    // If lines limit is specified and no offset was given, return only the last N lines
    if let (Some(limit), None) = (lines, from_offset) {
        let limit = limit as usize;
        let all_lines: Vec<&str> = content.lines().collect();
        if all_lines.len() > limit {
            content = all_lines[all_lines.len() - limit..].join("\n");
        }
    }

    Ok(WorkerLogResponse {
        content,
        byte_offset: file_size,
        file_size,
        exists: true,
    })
}

/// Get worker log file path
///
/// Returns the absolute path to the worker's log file for use with
/// file system watchers or external tools.
#[tauri::command]
pub async fn get_worker_log_path(run_name: String, worker_name: String) -> Result<String, String> {
    let run_dir = config::run_dir(&run_name);
    let files = crate::core::Files::new(&run_dir);
    let log_path = files.worker_log(&worker_name);
    Ok(log_path.to_string_lossy().to_string())
}

/// Parse log content and extract tool activity markers
///
/// Parses `[tool:name]` and `[/tool]` markers from Claude Code output.
#[tauri::command]
pub async fn parse_worker_log(content: String) -> Result<Vec<ParsedLogLine>, String> {
    let tool_start_re =
        regex::Regex::new(r"\[tool:([^\]]+)\]").map_err(|e| format!("Invalid regex: {}", e))?;
    let tool_end_re =
        regex::Regex::new(r"\[/tool\]").map_err(|e| format!("Invalid regex: {}", e))?;

    let parsed: Vec<ParsedLogLine> = content
        .lines()
        .map(|line| {
            let is_tool_start = tool_start_re.is_match(line);
            let is_tool_end = tool_end_re.is_match(line);
            let tool_name = if is_tool_start {
                tool_start_re.captures(line).map(|c| c[1].to_string())
            } else {
                None
            };

            ParsedLogLine {
                text: line.to_string(),
                is_tool_start,
                is_tool_end,
                tool_name,
            }
        })
        .collect();

    Ok(parsed)
}

/// Get eval log file content
#[tauri::command]
pub async fn get_eval_log(
    run_name: String,
    lines: Option<u32>,
    from_offset: Option<u64>,
) -> Result<WorkerLogResponse, String> {
    use std::io::{Read, Seek, SeekFrom};

    let run_dir = config::run_dir(&run_name);
    let files = crate::core::Files::new(&run_dir);
    let log_path = files.eval_log();

    if !log_path.exists() {
        return Ok(WorkerLogResponse {
            content: String::new(),
            byte_offset: 0,
            file_size: 0,
            exists: false,
        });
    }

    let metadata = std::fs::metadata(&log_path)
        .map_err(|e| format!("Failed to read eval log metadata: {}", e))?;
    let file_size = metadata.len();

    let mut file =
        std::fs::File::open(&log_path).map_err(|e| format!("Failed to open eval log: {}", e))?;

    let _start_offset = if let Some(offset) = from_offset {
        if offset < file_size {
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| format!("Failed to seek in eval log: {}", e))?;
            offset
        } else {
            return Ok(WorkerLogResponse {
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
        .map_err(|e| format!("Failed to read eval log: {}", e))?;

    if let (Some(limit), None) = (lines, from_offset) {
        let limit = limit as usize;
        let all_lines: Vec<&str> = content.lines().collect();
        if all_lines.len() > limit {
            content = all_lines[all_lines.len() - limit..].join("\n");
        }
    }

    Ok(WorkerLogResponse {
        content,
        byte_offset: file_size,
        file_size,
        exists: true,
    })
}

/// Get a specific eval's log file content by path
#[tauri::command]
pub async fn get_eval_log_by_path(
    run_name: String,
    log_file: String,
) -> Result<WorkerLogResponse, String> {
    use std::io::Read;
    use std::path::PathBuf;

    let run_dir = config::run_dir(&run_name);

    // Security: ensure the log file is within the run directory
    let log_path = PathBuf::from(&log_file);
    let canonical_run_dir = run_dir
        .canonicalize()
        .map_err(|e| format!("Failed to resolve run directory: {}", e))?;

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
        return Ok(WorkerLogResponse {
            content: String::new(),
            byte_offset: 0,
            file_size: 0,
            exists: false,
        });
    }

    let metadata = std::fs::metadata(&canonical_log_path)
        .map_err(|e| format!("Failed to read log metadata: {}", e))?;
    let file_size = metadata.len();

    let mut file = std::fs::File::open(&canonical_log_path)
        .map_err(|e| format!("Failed to open log file: {}", e))?;

    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| format!("Failed to read log file: {}", e))?;

    Ok(WorkerLogResponse {
        content,
        byte_offset: file_size,
        file_size,
        exists: true,
    })
}

/// Get history entries for a run
#[tauri::command]
pub async fn get_history(
    run_name: String,
    limit: Option<u32>,
) -> Result<Vec<HistoryEntry>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let limit = limit.unwrap_or(100) as i64;
    let core_history = state
        .get_history(limit)
        .map_err(|e| format!("Failed to get history: {}", e))?;

    let history = core_history
        .into_iter()
        .map(|h| HistoryEntry {
            id: h.id as u32,
            timestamp: h.timestamp,
            action: h.action,
            detail: h.detail,
        })
        .collect();

    Ok(history)
}

/// Get the eval spec (eval.md) content for a run
#[tauri::command]
pub async fn get_eval_spec(run_name: String) -> Result<Option<String>, String> {
    let run_dir = config::run_dir(&run_name);
    let eval_spec_path = run_dir.join("eval.md");

    if !eval_spec_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&eval_spec_path)
        .map_err(|e| format!("Failed to read eval spec: {}", e))?;

    Ok(Some(content))
}

/// Get evals for a run
#[tauri::command]
pub async fn get_evals(run_name: String) -> Result<Vec<Eval>, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let core_evals = state
        .get_evals(100)
        .map_err(|e| format!("Failed to get evals: {}", e))?;

    let evals = core_evals
        .into_iter()
        .map(|e| {
            let status = match e.status {
                crate::core::state::EvalStatus::Running => EvalStatus::Running,
                crate::core::state::EvalStatus::Passed => EvalStatus::Passed,
                crate::core::state::EvalStatus::Failed => EvalStatus::Failed,
            };

            Eval {
                id: e.id as u32,
                branch: e.branch.clone(),
                eval_name: e.eval_name,
                status,
                feedback: e.feedback,
                log_file: e.log_file,
                started_at: e.started_at,
                finished_at: e.finished_at,
                sheep_config: SheepConfig::for_eval(e.id as u32),
            }
        })
        .collect();

    Ok(evals)
}
