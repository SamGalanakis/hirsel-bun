//! Asset management for hirsel runs.
//!
//! Copies files (images, etc.) into a run's assets/ directory for use in specs and evals.

use std::fs;
use std::path::Path;

use crate::core::config::runs_dir;
use crate::core::files::Files;

/// Add assets to a run's assets directory.
///
/// Copies files to the run's assets/ folder, handling name conflicts by appending numbers.
pub fn run_asset(run_name: &str, paths: &[String]) -> Result<Vec<String>, String> {
    let run_dir = runs_dir().join(run_name);
    if !run_dir.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let files = Files::new(&run_dir);
    let assets_dir = files.assets();

    // Create assets directory if it doesn't exist
    fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    let mut added_files = Vec::new();

    for path_str in paths {
        let source_path = Path::new(path_str);

        if !source_path.exists() {
            return Err(format!("File not found: {}", path_str));
        }

        if !source_path.is_file() {
            return Err(format!("Not a file: {}", path_str));
        }

        // Get the filename
        let filename = source_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("Invalid filename: {}", path_str))?;

        // Find a unique name if file already exists
        let dest_filename = find_unique_filename(&assets_dir, filename);
        let dest_path = assets_dir.join(&dest_filename);

        // Copy the file
        fs::copy(source_path, &dest_path)
            .map_err(|e| format!("Failed to copy '{}': {}", path_str, e))?;

        added_files.push(dest_filename);
    }

    Ok(added_files)
}

/// Find a unique filename in the directory.
/// If "image.png" exists, tries "image-1.png", "image-2.png", etc.
fn find_unique_filename(dir: &Path, filename: &str) -> String {
    let dest = dir.join(filename);
    if !dest.exists() {
        return filename.to_string();
    }

    // Split into stem and extension
    let path = Path::new(filename);
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
