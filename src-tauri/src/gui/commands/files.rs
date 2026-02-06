//! File-related commands
//!
//! Commands for reading/writing spec and eval files, and managing assets.

use crate::core::config;

/// Read the spec.md file for a run
#[tracing::instrument]
#[tauri::command]
pub async fn read_spec_file(run_name: String) -> Result<String, String> {
    let spec_path = config::run_dir(&run_name).join("spec.md");
    if !spec_path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&spec_path).map_err(|e| format!("Failed to read spec file: {}", e))
}

/// Write the spec.md file for a run
#[tracing::instrument(skip(content))]
#[tauri::command]
pub async fn write_spec_file(run_name: String, content: String) -> Result<(), String> {
    let spec_path = config::run_dir(&run_name).join("spec.md");
    std::fs::write(&spec_path, &content).map_err(|e| format!("Failed to write spec file: {}", e))
}

/// Read the eval.md file for a run
#[tracing::instrument]
#[tauri::command]
pub async fn read_eval_file(run_name: String) -> Result<String, String> {
    let eval_path = config::run_dir(&run_name).join("eval.md");
    if !eval_path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&eval_path).map_err(|e| format!("Failed to read eval file: {}", e))
}

/// Write the eval.md file for a run
#[tracing::instrument(skip(content))]
#[tauri::command]
pub async fn write_eval_file(run_name: String, content: String) -> Result<(), String> {
    let eval_path = config::run_dir(&run_name).join("eval.md");
    std::fs::write(&eval_path, &content).map_err(|e| format!("Failed to write eval file: {}", e))
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
    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    // Find a unique filename
    let dest_filename = find_unique_asset_filename(&assets_dir, &filename);
    let dest_path = assets_dir.join(&dest_filename);

    // Write the file
    std::fs::write(&dest_path, &data).map_err(|e| format!("Failed to write asset: {}", e))?;

    Ok(dest_filename)
}

/// Find a unique filename in the assets directory
fn find_unique_asset_filename(dir: &std::path::Path, filename: &str) -> String {
    let dest = dir.join(filename);
    if !dest.exists() {
        return filename.to_string();
    }

    let path = std::path::Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    let ext = path.extension().and_then(|s| s.to_str());

    let mut counter = 1;
    loop {
        let new_name = match ext {
            Some(e) => format!("{}-{}.{}", stem, counter, e),
            None => format!("{}-{}", stem, counter),
        };

        if !dir.join(&new_name).exists() {
            return new_name;
        }
        counter += 1;
    }
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
    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    // Find a unique filename
    let dest_filename = find_unique_asset_filename(&assets_dir, &filename);
    let dest_path = assets_dir.join(&dest_filename);

    // Copy the file
    std::fs::copy(&source_path, &dest_path).map_err(|e| format!("Failed to copy asset: {}", e))?;

    Ok(dest_filename)
}

/// Open the assets folder for a run in the system file browser
#[tracing::instrument]
#[tauri::command]
pub async fn open_assets_folder(run_name: String) -> Result<(), String> {
    use crate::core::files::Files;
    use std::process::Command;

    let run_dir = config::run_dir(&run_name);
    if !run_dir.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let files = Files::new(&run_dir);
    let assets_dir = files.assets();

    // Create assets directory if it doesn't exist
    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    // Open in system file browser (cross-platform)
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&assets_dir)
            .spawn()
            .map_err(|e| format!("Failed to open assets folder: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(&assets_dir)
            .spawn()
            .map_err(|e| format!("Failed to open assets folder: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(&assets_dir)
            .spawn()
            .map_err(|e| format!("Failed to open assets folder: {}", e))?;
    }

    Ok(())
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
    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    // Find a unique filename
    let dest_filename = find_unique_asset_filename(&assets_dir, &filename);
    let dest_path = assets_dir.join(&dest_filename);

    // Write the file
    std::fs::write(&dest_path, &data).map_err(|e| format!("Failed to write asset: {}", e))?;

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
    use std::process::Command;

    let assets_dir = config::project_assets_dir(project_id);

    // Create assets directory if it doesn't exist
    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    // Open in system file browser (cross-platform)
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&assets_dir)
            .spawn()
            .map_err(|e| format!("Failed to open assets folder: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(&assets_dir)
            .spawn()
            .map_err(|e| format!("Failed to open assets folder: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(&assets_dir)
            .spawn()
            .map_err(|e| format!("Failed to open assets folder: {}", e))?;
    }

    Ok(())
}
