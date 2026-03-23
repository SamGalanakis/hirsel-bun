//! Project asset commands.

use super::ResultExt;
use crate::core::config;

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
