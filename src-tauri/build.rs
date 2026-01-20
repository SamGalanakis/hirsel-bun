use std::process::Command;

fn main() {
    // Get git commit SHA
    let git_sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Get git dirty status
    let git_dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let git_version = if git_dirty {
        format!("{}-dirty", git_sha)
    } else {
        git_sha
    };

    // Set environment variables for compilation
    println!("cargo:rustc-env=HIRSEL_GIT_SHA={}", git_version);
    println!(
        "cargo:rustc-env=HIRSEL_BUILD_DATE={}",
        chrono::Utc::now().format("%Y-%m-%d")
    );

    // Rebuild if git HEAD changes
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");

    #[cfg(feature = "gui")]
    tauri_build::build();
}
