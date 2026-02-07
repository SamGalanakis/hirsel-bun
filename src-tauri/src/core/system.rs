//! System utilities — cross-platform helpers for file operations and paths

/// Open a path in the system file browser
pub fn open_in_file_browser(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(target_os = "windows")]
    let cmd = "explorer";

    std::process::Command::new(cmd)
        .arg(path)
        .spawn()
        .map_err(|e| format!("Failed to open: {}", e))?;
    Ok(())
}

/// If a remote looks like a local filesystem path, convert to `file://` URL.
pub fn normalise_local_remote(url: &str) -> String {
    if url.starts_with('/') {
        format!("file://{}", url)
    } else if url.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            let expanded = url.replacen('~', &home.to_string_lossy(), 1);
            format!("file://{}", expanded)
        } else {
            url.to_string()
        }
    } else {
        url.to_string()
    }
}

/// Check whether a remote URL points to a local path.
pub fn is_local_remote(url: &str) -> bool {
    url.starts_with('/') || url.starts_with('~') || url.starts_with("file://")
}

/// Find a unique filename in a directory, appending `-1`, `-2`, etc. if needed.
pub fn find_unique_asset_filename(dir: &std::path::Path, filename: &str) -> String {
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
