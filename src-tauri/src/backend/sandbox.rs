use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::mpsc;

pub const DEFAULT_WORKER_IMAGE: &str = "hirsel-worker:local";
const WORKER_IMAGE_BUILD_LABEL: &str = "org.hirsel.worker-build";

fn default_image() -> String {
    DEFAULT_WORKER_IMAGE.to_string()
}

/// Single execution configuration for Hirsel coding agents.
///
/// Agents always run in Docker containers and enter the project through Nix.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone)]
pub struct SandboxImageProgress {
    pub progress: f64,
    pub detail: String,
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

fn classify_worker_build_progress(line: &str) -> Option<SandboxImageProgress> {
    let lower = line.to_ascii_lowercase();
    let progress = if lower.contains("load build definition from deploy/worker.dockerfile")
        || lower.contains("from rust:1-bookworm")
    {
        Some((0.18, "Starting the worker image build.".to_string()))
    } else if lower.contains("pkg-config")
        && lower.contains("libssl-dev")
        && lower.contains("apt-get install")
    {
        Some((0.32, "Installing Rust builder dependencies.".to_string()))
    } else if lower.contains("copy src-tauri/cargo.toml") || lower.contains("copy src-tauri/src") {
        Some((
            0.48,
            "Copying the worker source into the build context.".to_string(),
        ))
    } else if lower.contains("cargo build --release --locked --bin hirsel-worker") {
        Some((0.72, "Compiling `hirsel-worker`.".to_string()))
    } else if lower.contains("from ubuntu:24.04") {
        Some((0.84, "Preparing the runtime image.".to_string()))
    } else if lower.contains("ca-certificates")
        && lower.contains("libssl3")
        && lower.contains("apt-get install")
    {
        Some((0.9, "Installing runtime dependencies.".to_string()))
    } else if lower.contains("copy --from=builder") {
        Some((
            0.95,
            "Copying the worker binary into the runtime image.".to_string(),
        ))
    } else if lower.contains("exporting to image") || lower.contains("naming to") {
        Some((0.98, "Finalizing the worker image.".to_string()))
    } else {
        None
    }?;

    Some(SandboxImageProgress {
        progress: progress.0,
        detail: progress.1,
    })
}

async fn forward_pipe_lines<R>(reader: R, tx: mpsc::UnboundedSender<String>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let _ = tx.send(line);
    }
}

async fn build_default_worker_image_with_progress<F, Fut>(mut report: F) -> Result<(), String>
where
    F: FnMut(SandboxImageProgress) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let repo_root = resolve_worker_build_root().ok_or_else(|| {
        format!(
            "The default worker image '{}' is missing, and Hirsel could not find deploy/worker.Dockerfile to build it automatically. Run Hirsel from the repo checkout, build the image manually, or override the project sandbox image with one that already contains hirsel-worker.",
            DEFAULT_WORKER_IMAGE
        )
    })?;
    let label = worker_build_label();
    report(SandboxImageProgress {
        progress: 0.14,
        detail: format!(
            "Building `{}` from `deploy/worker.Dockerfile`.",
            DEFAULT_WORKER_IMAGE
        ),
    })
    .await?;

    let mut child = TokioCommand::new("docker")
        .current_dir(&repo_root)
        .args([
            "build",
            "--progress=plain",
            "-f",
            "deploy/worker.Dockerfile",
            "--build-arg",
            &format!("HIRSEL_WORKER_BUILD_LABEL={label}"),
            "-t",
            DEFAULT_WORKER_IMAGE,
            ".",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            humanize_docker_error(&format!("failed to build worker image: {}", error))
        })?;

    let (tx, mut rx) = mpsc::unbounded_channel();
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(forward_pipe_lines(stdout, tx.clone()));
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(forward_pipe_lines(stderr, tx.clone()));
    }
    drop(tx);

    let mut recent_lines = VecDeque::with_capacity(24);
    let mut last_progress = 0.14;
    let mut last_detail = format!(
        "Building `{}` from `deploy/worker.Dockerfile`.",
        DEFAULT_WORKER_IMAGE
    );

    loop {
        while let Ok(line) = rx.try_recv() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                if recent_lines.len() == 24 {
                    recent_lines.pop_front();
                }
                recent_lines.push_back(trimmed.to_string());
            }
            if let Some(update) = classify_worker_build_progress(trimmed) {
                if update.progress > last_progress || update.detail != last_detail {
                    last_progress = update.progress.max(last_progress);
                    last_detail = update.detail.clone();
                    report(update).await?;
                }
            }
        }

        if let Some(status) = child.try_wait().map_err(|error| {
            humanize_docker_error(&format!("failed to wait for docker build: {}", error))
        })? {
            if status.success() {
                return Ok(());
            }
            let message = if recent_lines.is_empty() {
                "docker build failed".to_string()
            } else {
                recent_lines.into_iter().collect::<Vec<_>>().join("\n")
            };
            return Err(humanize_docker_error(&message));
        }

        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        let next_progress = (last_progress + 0.02).min(0.92);
        if next_progress > last_progress {
            last_progress = next_progress;
            report(SandboxImageProgress {
                progress: last_progress,
                detail: last_detail.clone(),
            })
            .await?;
        }
    }
}

pub async fn ensure_sandbox_image_available_with_progress<F, Fut>(
    image: &str,
    mut report: F,
) -> Result<(), String>
where
    F: FnMut(SandboxImageProgress) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    report(SandboxImageProgress {
        progress: 0.06,
        detail: "Checking Docker availability.".to_string(),
    })
    .await?;
    ensure_docker_available()?;

    if image != DEFAULT_WORKER_IMAGE {
        report(SandboxImageProgress {
            progress: 1.0,
            detail: format!("Using configured worker image `{}`.", image),
        })
        .await?;
        return Ok(());
    }

    report(SandboxImageProgress {
        progress: 0.1,
        detail: format!("Inspecting local worker image `{}`.", image),
    })
    .await?;

    let current_label = current_worker_image_label(image)?;
    if current_label.as_deref() == Some(worker_build_label().as_str()) {
        report(SandboxImageProgress {
            progress: 1.0,
            detail: format!("Worker image `{}` is ready.", image),
        })
        .await?;
        return Ok(());
    }

    build_default_worker_image_with_progress(&mut report).await?;
    report(SandboxImageProgress {
        progress: 1.0,
        detail: format!("Worker image `{}` is ready.", image),
    })
    .await?;
    Ok(())
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

pub async fn ensure_sandbox_image_available(image: &str) -> Result<(), String> {
    ensure_sandbox_image_available_with_progress(image, |_| async { Ok(()) }).await
}
