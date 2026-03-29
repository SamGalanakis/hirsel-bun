use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

pub const DEFAULT_WORKER_IMAGE: &str = "hirsel-worker:local";
const WORKER_IMAGE_BUILD_LABEL: &str = "org.hirsel.worker-build";

fn default_image() -> String {
    DEFAULT_WORKER_IMAGE.to_string()
}

/// Single execution configuration for Hirsel coding agents.
///
/// Agents always run in Docker containers and enter the project through Nix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    #[serde(default = "default_image")]
    pub image: String,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self::docker_nix()
    }
}

impl SandboxConfig {
    pub fn docker_nix() -> Self {
        Self {
            image: default_image(),
        }
    }
}

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("Container runtime failed: {0}")]
    Container(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),
}

fn best_command_output(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }
    "docker command failed".to_string()
}

fn docker_output(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("docker")
        .args(args)
        .output()
        .map_err(|error| humanize_docker_error(&format!("failed to run docker: {}", error)))
}

fn worker_build_label() -> String {
    format!("{}-{}", crate::version::VERSION, crate::version::GIT_SHA)
}

fn find_repo_root_with_worker_dockerfile(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|candidate| {
        let dockerfile = candidate.join("deploy").join("worker.Dockerfile");
        if dockerfile.is_file() {
            Some(candidate.to_path_buf())
        } else {
            None
        }
    })
}

fn resolve_worker_build_root() -> Option<PathBuf> {
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(root) = find_repo_root_with_worker_dockerfile(&cwd) {
            return Some(root);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = find_repo_root_with_worker_dockerfile(&exe) {
            return Some(root);
        }
    }

    None
}

fn current_worker_image_label(image: &str) -> Result<Option<String>, String> {
    let format = format!(
        "{{{{index .Config.Labels \"{}\"}}}}",
        WORKER_IMAGE_BUILD_LABEL
    );
    let output = docker_output(&["image", "inspect", "--format", &format, image])?;
    if output.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ));
    }

    let message = best_command_output(&output);
    if message.to_ascii_lowercase().contains("no such object") {
        return Ok(None);
    }

    Err(humanize_docker_error(&message))
}

fn build_default_worker_image() -> Result<(), String> {
    let repo_root = resolve_worker_build_root().ok_or_else(|| {
        format!(
            "The default worker image '{}' is missing, and Hirsel could not find deploy/worker.Dockerfile to build it automatically. Run Hirsel from the repo checkout, build the image manually, or override the project sandbox image with one that already contains hirsel-worker.",
            DEFAULT_WORKER_IMAGE
        )
    })?;
    let label = worker_build_label();
    let output = Command::new("docker")
        .current_dir(&repo_root)
        .args([
            "build",
            "-f",
            "deploy/worker.Dockerfile",
            "--build-arg",
            &format!("HIRSEL_WORKER_BUILD_LABEL={label}"),
            "-t",
            DEFAULT_WORKER_IMAGE,
            ".",
        ])
        .output()
        .map_err(|error| {
            humanize_docker_error(&format!("failed to build worker image: {}", error))
        })?;

    if output.status.success() {
        return Ok(());
    }

    Err(humanize_docker_error(&best_command_output(&output)))
}

pub fn humanize_docker_error(error: &str) -> String {
    let trimmed = error.trim();
    let lower = trimmed.to_ascii_lowercase();

    if lower.contains("cannot connect to the docker daemon")
        || lower.contains("is the docker daemon running")
        || lower.contains("error during connect")
    {
        return "Docker is installed but the daemon is not reachable. Start Docker and try again."
            .to_string();
    }

    if lower.contains("failed to run docker: no such file or directory")
        || lower.contains("failed to launch container runtime: no such file or directory")
    {
        return "Docker is not installed on this machine, so Hirsel cannot start coding containers yet."
            .to_string();
    }

    trimmed.to_string()
}

pub fn ensure_docker_available() -> Result<(), String> {
    let output = docker_output(&["info", "--format", "{{json .ServerVersion}}"])?;

    if output.status.success() {
        return Ok(());
    }

    Err(humanize_docker_error(&best_command_output(&output)))
}

pub fn ensure_sandbox_image_available(image: &str) -> Result<(), String> {
    ensure_docker_available()?;

    if image != DEFAULT_WORKER_IMAGE {
        return Ok(());
    }

    let current_label = current_worker_image_label(image)?;
    if current_label.as_deref() == Some(worker_build_label().as_str()) {
        return Ok(());
    }

    build_default_worker_image()
}
