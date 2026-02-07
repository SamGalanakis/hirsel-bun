//! File-related commands
//!
//! Commands for reading/writing spec and eval files, and managing assets.

use super::ResultExt;
use crate::core::config;

/// Read the spec.md file for a run
#[tracing::instrument]
#[tauri::command]
pub async fn read_spec_file(run_name: String) -> Result<String, String> {
    let spec_path = config::run_dir(&run_name).join("spec.md");
    if !spec_path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&spec_path).context("Failed to read spec file")
}

/// Write the spec.md file for a run
#[tracing::instrument(skip(content))]
#[tauri::command]
pub async fn write_spec_file(run_name: String, content: String) -> Result<(), String> {
    let spec_path = config::run_dir(&run_name).join("spec.md");
    std::fs::write(&spec_path, &content).context("Failed to write spec file")
}

/// Read the eval.md file for a run
#[tracing::instrument]
#[tauri::command]
pub async fn read_eval_file(run_name: String) -> Result<String, String> {
    let eval_path = config::run_dir(&run_name).join("eval.md");
    if !eval_path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&eval_path).context("Failed to read eval file")
}

/// Write the eval.md file for a run
#[tracing::instrument(skip(content))]
#[tauri::command]
pub async fn write_eval_file(run_name: String, content: String) -> Result<(), String> {
    let eval_path = config::run_dir(&run_name).join("eval.md");
    std::fs::write(&eval_path, &content).context("Failed to write eval file")
}

/// Save an asset file (image, etc.) to a run's assets directory
///
/// Returns the filename that was saved (may differ from original if name conflict)
#[tracing::instrument(skip(data))]
#[tauri::command]
pub async fn save_asset(
    run_name: String,
    filename: String,
    data: Vec<u8>,
) -> Result<String, String> {
    use crate::core::files::Files;

    let run_dir = config::run_dir(&run_name);
    if !run_dir.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let files = Files::new(&run_dir);
    let assets_dir = files.assets();

    // Create assets directory if it doesn't exist
    std::fs::create_dir_all(&assets_dir).context("Failed to create assets directory")?;

    // Find a unique filename
    let dest_filename = crate::core::system::find_unique_asset_filename(&assets_dir, &filename);
    let dest_path = assets_dir.join(&dest_filename);

    // Write the file
    std::fs::write(&dest_path, &data).context("Failed to write asset")?;

    Ok(dest_filename)
}

/// Import a file from a filesystem path into a run's assets directory
/// Used by drag-and-drop from native file manager
#[tracing::instrument]
#[tauri::command]
pub async fn import_asset_from_path(run_name: String, file_path: String) -> Result<String, String> {
    use crate::core::files::Files;

    let run_dir = config::run_dir(&run_name);
    if !run_dir.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let source_path = std::path::PathBuf::from(&file_path);
    if !source_path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    if !source_path.is_file() {
        return Err(format!("Not a file: {}", file_path));
    }

    let filename = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let files = Files::new(&run_dir);
    let assets_dir = files.assets();

    // Create assets directory if it doesn't exist
    std::fs::create_dir_all(&assets_dir).context("Failed to create assets directory")?;

    // Find a unique filename
    let dest_filename = crate::core::system::find_unique_asset_filename(&assets_dir, &filename);
    let dest_path = assets_dir.join(&dest_filename);

    // Copy the file
    std::fs::copy(&source_path, &dest_path).context("Failed to copy asset")?;

    Ok(dest_filename)
}

/// Open the assets folder for a run in the system file browser
#[tracing::instrument]
#[tauri::command]
pub async fn open_assets_folder(run_name: String) -> Result<(), String> {
    use crate::core::files::Files;

    let run_dir = config::run_dir(&run_name);
    if !run_dir.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let files = Files::new(&run_dir);
    let assets_dir = files.assets();

    // Create assets directory if it doesn't exist
    std::fs::create_dir_all(&assets_dir).context("Failed to create assets directory")?;

    crate::core::system::open_in_file_browser(&assets_dir)
}

/// Get the assets base URL for a run (for rendering images in markdown)
#[tracing::instrument]
#[tauri::command]
pub async fn get_assets_path(run_name: String) -> Result<String, String> {
    use crate::core::files::Files;

    let run_dir = config::run_dir(&run_name);
    let files = Files::new(&run_dir);
    let assets_dir = files.assets();

    Ok(assets_dir.to_string_lossy().to_string())
}

// =============================================================================
// Project Asset Commands
// =============================================================================

/// Save an asset file to a project's assets directory
///
/// Returns the filename that was saved (may differ from original if name conflict)
#[tracing::instrument(skip(data))]
#[tauri::command]
pub async fn save_project_asset(
    project_id: i64,
    filename: String,
    data: Vec<u8>,
) -> Result<String, String> {
    let assets_dir = config::project_assets_dir(project_id);

    // Create assets directory if it doesn't exist
    std::fs::create_dir_all(&assets_dir).context("Failed to create assets directory")?;

    // Find a unique filename
    let dest_filename = crate::core::system::find_unique_asset_filename(&assets_dir, &filename);
    let dest_path = assets_dir.join(&dest_filename);

    // Write the file
    std::fs::write(&dest_path, &data).context("Failed to write asset")?;

    Ok(dest_filename)
}

/// Get the assets directory path for a project
#[tracing::instrument]
#[tauri::command]
pub async fn get_project_assets_path(project_id: i64) -> Result<String, String> {
    let assets_dir = config::project_assets_dir(project_id);
    Ok(assets_dir.to_string_lossy().to_string())
}

/// Open the assets folder for a project in the system file browser
#[tracing::instrument]
#[tauri::command]
pub async fn open_project_assets_folder(project_id: i64) -> Result<(), String> {
    let assets_dir = config::project_assets_dir(project_id);

    // Create assets directory if it doesn't exist
    std::fs::create_dir_all(&assets_dir).context("Failed to create assets directory")?;

    crate::core::system::open_in_file_browser(&assets_dir)
}
