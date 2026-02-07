//! IDE-related commands
//!
//! Commands for opening project staging directories in the user's preferred IDE.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use super::ResultExt;
use crate::core::config;

/// Result of opening in IDE
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenIdeResult {
    pub success: bool,
    pub ide_used: String,
    pub path_opened: String,
    pub was_downloaded: bool,
}

/// A detected editor with its executable path
#[derive(Debug, Clone)]
struct FoundEditor {
    name: String,
    path: String,
}

/// Editor info with candidate installation paths
struct EditorInfo {
    name: &'static str,
    paths: &'static [&'static str],
}

// Editor registries for each platform (following GitHub Desktop pattern)
#[cfg(target_os = "linux")]
const EDITORS: &[EditorInfo] = &[
    EditorInfo {
        name: "cursor",
        paths: &["/usr/bin/cursor", "/snap/bin/cursor", "cursor"],
    },
    EditorInfo {
        name: "code",
        paths: &["/usr/bin/code", "/snap/bin/code", "code"],
    },
    EditorInfo {
        name: "zed",
        paths: &["/usr/bin/zed", "zed"],
    },
    EditorInfo {
        name: "nvim",
        paths: &["/usr/bin/nvim", "nvim"],
    },
];

#[cfg(target_os = "macos")]
const EDITORS: &[EditorInfo] = &[
    EditorInfo {
        name: "cursor",
        paths: &[
            "/Applications/Cursor.app/Contents/Resources/app/bin/cursor",
            "cursor",
        ],
    },
    EditorInfo {
        name: "code",
        paths: &[
            "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
            "code",
        ],
    },
    EditorInfo {
        name: "zed",
        paths: &["/Applications/Zed.app/Contents/MacOS/cli", "zed"],
    },
];

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const EDITORS: &[EditorInfo] = &[];

/// Cache for detected editors (avoids repeated filesystem scans)
static EDITOR_CACHE: OnceLock<Vec<FoundEditor>> = OnceLock::new();

/// Detect available editors on the system
fn detect_available_editors() -> &'static Vec<FoundEditor> {
    EDITOR_CACHE.get_or_init(|| {
        EDITORS
            .iter()
            .filter_map(|editor| {
                editor.paths.iter().find_map(|path| {
                    // Check if it's an absolute path that exists
                    if path.starts_with('/') && Path::new(path).exists() {
                        return Some(FoundEditor {
                            name: editor.name.to_string(),
                            path: path.to_string(),
                        });
                    }
                    // Check if it's in PATH using which
                    if let Ok(output) = Command::new("which").arg(path).output() {
                        if output.status.success() {
                            let found_path =
                                String::from_utf8_lossy(&output.stdout).trim().to_string();
                            if !found_path.is_empty() {
                                return Some(FoundEditor {
                                    name: editor.name.to_string(),
                                    path: found_path,
                                });
                            }
                        }
                    }
                    None
                })
            })
            .collect()
    })
}

/// Get the preferred editor (from config or first available)
fn get_preferred_editor() -> Result<FoundEditor, String> {
    let available = detect_available_editors();

    if available.is_empty() {
        return Err(
            "No IDE found. Install VS Code, Cursor, or Zed and ensure they're in PATH.".to_string(),
        );
    }

    // Check if user has a preference
    let (cfg, _) = config::Config::load().str_err()?;
    if let Some(ref preferred) = cfg.preferred_ide {
        if let Some(editor) = available.iter().find(|e| &e.name == preferred) {
            return Ok(editor.clone());
        }
        tracing::warn!(
            "Preferred IDE '{}' not found, falling back to first available",
            preferred
        );
    }

    // Return first available
    Ok(available[0].clone())
}

/// Launch an editor with the given path
fn launch_editor(editor: &FoundEditor, target_path: &Path) -> Result<(), String> {
    let target_str = target_path
        .to_str()
        .ok_or_else(|| "Invalid path encoding".to_string())?;

    #[cfg(target_os = "macos")]
    {
        // macOS: use 'open -a' for .app bundles, direct spawn otherwise
        if editor.path.contains(".app") {
            Command::new("open")
                .args(["-a", &editor.path, target_str])
                .spawn()
                .context(&format!("Failed to launch {}", editor.name))?;
        } else {
            Command::new(&editor.path)
                .arg(target_str)
                .spawn()
                .context(&format!("Failed to launch {}", editor.name))?;
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        Command::new(&editor.path)
            .arg(target_str)
            .spawn()
            .context(&format!("Failed to launch {}", editor.name))?;
    }

    Ok(())
}

/// Get the staging path for a run
///
/// Returns the `work/staging/` directory where workers push their changes.
/// This is the canonical working directory containing the current state of the code.
///
/// Architecture:
/// - `workspace/` is the INPUT (initial project state, e.g., empty git repo for greenfield)
/// - `work/staging/` is the OUTPUT (where workers actually write code)
fn get_staging_path(run_name: &str) -> Result<PathBuf, String> {
    let run_dir = config::run_dir(run_name);
    let staging_dir = run_dir.join("work").join("staging");

    if !staging_dir.exists() {
        return Err(format!(
            "No workspace found for run '{}'. Workers may not have started yet.",
            run_name
        ));
    }

    Ok(staging_dir)
}

/// Open the staging directory for a run in the user's preferred IDE
#[tracing::instrument]
#[tauri::command]
pub async fn open_in_ide(run_name: String) -> Result<OpenIdeResult, String> {
    // Get the staging path for this run
    let staging_path = get_staging_path(&run_name)?;

    // Get the preferred/available editor
    let editor = get_preferred_editor()?;

    // Launch the editor
    launch_editor(&editor, &staging_path)?;

    tracing::info!(
        "Opened {} in {} for run {}",
        staging_path.display(),
        editor.name,
        run_name
    );

    Ok(OpenIdeResult {
        success: true,
        ide_used: editor.name,
        path_opened: staging_path.display().to_string(),
        was_downloaded: false, // Local runs don't need download
    })
}
