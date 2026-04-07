use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use walkdir::WalkDir;

pub const DEFAULT_WORKER_IMAGE: &str = "hirsel-worker:local";
const WORKER_IMAGE_BUILD_LABEL: &str = "org.hirsel.worker-build";
const DEFAULT_WORKER_CARGO_PROFILE: &str = "release";

fn default_image() -> String {
    DEFAULT_WORKER_IMAGE.to_string()
}

/// Single execution configuration for Hirsel coding agents.
///
/// Agents always run in Docker containers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    #[serde(default = "default_image")]
    pub image: String,
}

impl Default for SandboxConfig {
    fn default() -> Self {
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

#[derive(Debug, Clone, Copy)]
struct StableFingerprint(u64);

impl StableFingerprint {
    fn new() -> Self {
        Self(0xcbf29ce484222325)
    }

    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }

    fn finish(self) -> String {
        format!("{:016x}", self.0)
    }
}

fn current_worker_source_fingerprint() -> Result<String, String> {
    let repo_root = resolve_worker_build_root()
        .ok_or_else(|| "failed to resolve worker build root".to_string())?;
    let src_tauri = repo_root.join("src-tauri");
    let mut files = vec![
        repo_root.join("deploy").join("worker.Dockerfile"),
        src_tauri.join("Cargo.toml"),
        src_tauri.join("Cargo.lock"),
        src_tauri.join("build.rs"),
    ];

    let src_dir = src_tauri.join("src");
    if src_dir.is_dir() {
        files.extend(
            WalkDir::new(&src_dir)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_file())
                .map(|entry| entry.into_path()),
        );
    }

    files.sort();

    let mut fingerprint = StableFingerprint::new();
    for path in files {
        if !path.is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(&repo_root)
            .map_err(|error| format!("failed to relativize '{}': {}", path.display(), error))?;
        fingerprint.update(rel.to_string_lossy().as_bytes());
        fingerprint.update(&[0]);
        fingerprint.update(
            &std::fs::read(&path)
                .map_err(|error| format!("failed to read '{}': {}", path.display(), error))?,
        );
        fingerprint.update(&[0xff]);
    }

    Ok(fingerprint.finish())
}

fn worker_build_label() -> String {
    let source = current_worker_source_fingerprint().unwrap_or_else(|_| "unknown".to_string());
    format!(
        "{}-{}-{}",
        crate::version::VERSION,
        crate::version::GIT_SHA,
        source
    )
}

pub fn worker_image_build_label() -> String {
    worker_build_label()
}

pub fn worker_image_cargo_profile() -> &'static str {
    match std::env::var("HIRSEL_WORKER_CARGO_PROFILE") {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "dev" | "debug" => "dev",
            "release" => "release",
            _ => DEFAULT_WORKER_CARGO_PROFILE,
        },
        Err(_) => DEFAULT_WORKER_CARGO_PROFILE,
    }
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

#[derive(Debug, Clone)]
pub struct SandboxImageProgress {
    pub progress: f64,
    pub detail: String,
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

#[cfg(feature = "host")]
pub(crate) fn current_worker_runtime_fingerprint(image: &str) -> Result<Option<String>, String> {
    let format = format!(
        "{{{{.Id}}}}|{{{{index .Config.Labels \"{}\"}}}}",
        WORKER_IMAGE_BUILD_LABEL
    );
    let output = docker_output(&["image", "inspect", "--format", &format, image])?;
    if output.status.success() {
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let Some((image_id, label)) = raw.split_once('|') else {
            return Ok(Some(raw));
        };
        let image_id = image_id.trim();
        let label = label.trim();
        if !label.is_empty() {
            return Ok(Some(format!("{image_id}|{label}")));
        }
        return Ok(Some(image_id.to_string()));
    }

    let message = best_command_output(&output);
    if message.to_ascii_lowercase().contains("no such object") {
        return Ok(None);
    }

    Err(humanize_docker_error(&message))
}

pub async fn ensure_sandbox_image_available(image: &str) -> Result<(), String> {
    ensure_docker_available()?;

    if image != DEFAULT_WORKER_IMAGE {
        return Ok(());
    }

    let current_label = current_worker_image_label(image)?;
    let expected_label = worker_image_build_label();
    if current_label.as_deref() == Some(expected_label.as_str()) {
        return Ok(());
    }

    let repo_hint = resolve_worker_build_root()
        .map(|root| format!(
            "Build it explicitly before starting runtimes: (cd {} && DOCKER_BUILDKIT=1 docker build -f deploy/worker.Dockerfile --build-arg HIRSEL_WORKER_BUILD_LABEL={} --build-arg HIRSEL_WORKER_CARGO_PROFILE={} -t {} .)",
            root.display(),
            expected_label,
            worker_image_cargo_profile(),
            image,
        ))
        .unwrap_or_else(|| format!(
            "Build the worker image explicitly and tag it as `{}` before starting runtimes.",
            image,
        ));

    match current_label {
        Some(found) => Err(format!(
            "Worker image `{}` is stale. Expected build label `{}`, found `{}`. {}",
            image, expected_label, found, repo_hint
        )),
        None => Err(format!(
            "Worker image `{}` is missing. {}",
            image, repo_hint
        )),
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
        progress: 0.1,
        detail: "Checking Docker availability.".to_string(),
    })
    .await?;
    ensure_docker_available()?;

    report(SandboxImageProgress {
        progress: 0.3,
        detail: format!("Inspecting worker image `{}`.", image),
    })
    .await?;

    ensure_sandbox_image_available(image).await?;

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
