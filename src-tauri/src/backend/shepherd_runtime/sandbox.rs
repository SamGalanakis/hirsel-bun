use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use git2::{ObjectType, Oid};

use super::rpc::{server_control_socket_path, wait_for_worker_socket};
use super::runtime::resolve_scope_workspace;
use super::session::{ShepherdScopeSession, ShepherdSessionStore};
use super::types::ShepherdScope;
use crate::backend::ensure_thread_checkout;
use crate::backend::project::ProjectStore;
use crate::backend::sandbox::{
    current_worker_runtime_fingerprint, ensure_sandbox_image_available, humanize_docker_error,
    SandboxConfig,
};

fn quote_shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) fn scope_key(scope: &ShepherdScope) -> String {
    match scope {
        ShepherdScope::General => "general".to_string(),
        ShepherdScope::Shepherd { project_id, .. } => format!("shepherd-{project_id}"),
        ShepherdScope::Thread { thread_id, .. } => format!("thread-{thread_id}"),
        ShepherdScope::Librarian { project_id, .. } => format!("librarian-{project_id}"),
    }
}

pub(crate) fn session_dir(scope: &ShepherdScope) -> PathBuf {
    crate::backend::config::hirsel_dir()
        .join("agent-sessions")
        .join(scope_key(scope))
}

fn container_session_dir(scope: &ShepherdScope) -> String {
    format!("/hirsel/agent-sessions/{}", scope_key(scope))
}

fn scope_file_path(scope: &ShepherdScope) -> PathBuf {
    session_dir(scope).join("scope.json")
}

fn container_scope_file(scope: &ShepherdScope) -> String {
    format!("{}/scope.json", container_session_dir(scope))
}

fn worker_socket_path(scope: &ShepherdScope) -> PathBuf {
    session_dir(scope).join("worker.sock")
}

fn container_worker_socket(scope: &ShepherdScope) -> String {
    format!("{}/worker.sock", container_session_dir(scope))
}

fn container_name(scope: &ShepherdScope) -> String {
    let raw = format!("hirsel-{}", scope_key(scope));
    raw.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn build_scope_runtime_script(scope: &ShepherdScope) -> Result<String, String> {
    let serve_cmd = format!(
        "hirsel-worker serve --scope-file {} --socket-path {}",
        quote_shell(&container_scope_file(scope)),
        quote_shell(&container_worker_socket(scope)),
    );
    let script = format!(
        r#"set -e
export HIRSEL_ROOT=/hirsel
export HOME=/tmp/home
export HIRSEL_SCOPE_WORKDIR=/work
export PATH="/usr/local/bin:$PATH"
mkdir -p "$HOME"
export HIRSEL_SERVER_RPC_SOCKET=/hirsel/server/control.sock
exec {}
"#,
        serve_cmd
    );
    Ok(script)
}

fn push_env(args: &mut Vec<String>, key: &str, value: &str) {
    if !value.trim().is_empty() {
        args.push("-e".to_string());
        args.push(format!("{key}={value}"));
    }
}

async fn current_env_fingerprint(_work_dir: &Path) -> Result<String, String> {
    let (config, _) = crate::backend::config::Config::load()
        .map_err(|error| format!("failed to load config: {}", error))?;
    let forwarded = crate::backend::credentials::load_forwarded_credentials().await;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\0llm\0");
    bytes.extend_from_slice(
        &serde_json::to_vec(&config.llm)
            .map_err(|error| format!("failed to serialize llm config fingerprint: {}", error))?,
    );
    bytes.extend_from_slice(b"\0forwarded\0");
    bytes.extend_from_slice(&serde_json::to_vec(&forwarded).map_err(|error| {
        format!(
            "failed to serialize forwarded credential fingerprint: {}",
            error
        )
    })?);

    let oid = Oid::hash_object(ObjectType::Blob, &bytes)
        .map_err(|error| format!("failed to fingerprint scope environment: {}", error))?;
    Ok(oid.to_string())
}

async fn load_sandbox_config(scope: &ShepherdScope) -> Result<SandboxConfig, String> {
    let (config, _) = crate::backend::config::Config::load()
        .map_err(|error| format!("failed to load sandbox config: {}", error))?;
    let mut sandbox = config.sandbox;

    let project_id = match scope {
        ShepherdScope::General => None,
        ShepherdScope::Shepherd { project_id, .. } => Some(*project_id),
        ShepherdScope::Thread { project_id, .. } => Some(*project_id),
        ShepherdScope::Librarian { project_id, .. } => Some(*project_id),
    };

    if let Some(project_id) = project_id {
        let store = ProjectStore::open()
            .await
            .map_err(|error| format!("failed to open project store: {}", error))?;
        let project = store
            .get_project(project_id)
            .await
            .map_err(|error| format!("failed to load project {}: {}", project_id, error))?;
        if let Some(image) = project
            .sandbox_image
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            sandbox.image = image.to_string();
        }
    }

    Ok(sandbox)
}

async fn prepare_scope_runtime(scope: &ShepherdScope) -> Result<(PathBuf, SandboxConfig), String> {
    if let ShepherdScope::Thread {
        project_id,
        thread_id,
        ..
    } = scope
    {
        let _ = ensure_thread_checkout(*project_id, thread_id).await?;
    }
    let work_dir = resolve_scope_workspace(scope).await?;
    let sandbox = load_sandbox_config(scope).await?;
    ensure_sandbox_image_available(&sandbox.image).await?;
    Ok((work_dir, sandbox))
}

pub(super) async fn validate_scope_runtime(scope: &ShepherdScope) -> Result<(), String> {
    prepare_scope_runtime(scope).await.map(|_| ())
}

fn docker_output(args: &[String]) -> Result<std::process::Output, String> {
    Command::new("docker")
        .args(args)
        .output()
        .map_err(|error| humanize_docker_error(&format!("failed to run docker: {}", error)))
}

fn docker_logs(container_name: &str) -> String {
    match docker_output(&["logs".to_string(), container_name.to_string()]) {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !stderr.is_empty() {
                stderr
            } else {
                stdout
            }
        }
        Err(error) => error,
    }
}

fn container_is_running(container_name: &str) -> Result<bool, String> {
    let output = docker_output(&[
        "inspect".to_string(),
        "-f".to_string(),
        "{{.State.Running}}".to_string(),
        container_name.to_string(),
    ])?;
    if !output.status.success() {
        return Err(humanize_docker_error(&best_output(&output)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
}

fn remove_container_if_present(container_name: &str) -> Result<(), String> {
    let output = docker_output(&[
        "rm".to_string(),
        "-f".to_string(),
        container_name.to_string(),
    ])?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let text = if !stderr.is_empty() { stderr } else { stdout };
    if text.to_ascii_lowercase().contains("no such container") {
        return Ok(());
    }
    Err(humanize_docker_error(&text))
}

fn best_output(output: &std::process::Output) -> String {
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

fn write_scope_file(scope: &ShepherdScope) -> Result<(), String> {
    let dir = session_dir(scope);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create session dir: {}", error))?;
    let scope_json = serde_json::to_vec_pretty(scope)
        .map_err(|error| format!("failed to serialize scope json: {}", error))?;
    std::fs::write(scope_file_path(scope), scope_json)
        .map_err(|error| format!("failed to write scope file: {}", error))
}

async fn start_scope_container(scope: &ShepherdScope) -> Result<ShepherdScopeSession, String> {
    let (work_dir, sandbox) = prepare_scope_runtime(scope).await?;
    let forwarded = crate::backend::credentials::load_forwarded_credentials().await;
    let env_fingerprint = current_env_fingerprint(&work_dir).await?;
    let runtime_fingerprint = current_worker_runtime_fingerprint(&sandbox.image)?;
    let hirsel_root = crate::backend::config::hirsel_dir();
    std::fs::create_dir_all(hirsel_root.join("server"))
        .map_err(|error| format!("failed to create hirsel server dir: {}", error))?;
    std::fs::create_dir_all(session_dir(scope).join("home"))
        .map_err(|error| format!("failed to create session home dir: {}", error))?;
    write_scope_file(scope)?;
    let socket_path = worker_socket_path(scope);
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }

    let container_name = container_name(scope);
    remove_container_if_present(&container_name)?;

    let script = build_scope_runtime_script(scope)?;
    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        container_name.clone(),
        "--add-host".to_string(),
        "host.docker.internal:host-gateway".to_string(),
        "--user".to_string(),
        format!("{}:{}", unsafe { libc::getuid() }, unsafe {
            libc::getgid()
        }),
        "-v".to_string(),
        format!("{}:/hirsel", hirsel_root.display()),
        "-v".to_string(),
        format!("{}:/work", work_dir.display()),
        "-v".to_string(),
        format!("{}:/tmp/home", session_dir(scope).join("home").display()),
        "-w".to_string(),
        "/work".to_string(),
    ];

    if let Some(value) = forwarded.openai_api_key.as_deref() {
        push_env(&mut args, "OPENAI_API_KEY", value);
    }
    if let Some(value) = forwarded.openrouter_api_key.as_deref() {
        push_env(&mut args, "OPENROUTER_API_KEY", value);
    }
    if let Some(value) = forwarded.tavily_api_key.as_deref() {
        push_env(&mut args, "TAVILY_API_KEY", value);
    }
    if let Some(value) = forwarded.github_token.as_deref() {
        push_env(&mut args, "GITHUB_TOKEN", value);
        push_env(&mut args, "GH_TOKEN", value);
    }
    if let Some(value) = forwarded.codex_access_token.as_deref() {
        push_env(&mut args, "CODEX_ACCESS_TOKEN", value);
    }
    if let Some(value) = forwarded.codex_refresh_token.as_deref() {
        push_env(&mut args, "CODEX_REFRESH_TOKEN", value);
    }
    if let Some(value) = forwarded.codex_expires_at.as_deref() {
        push_env(&mut args, "CODEX_EXPIRES_AT", value);
    }
    if let Some(value) = forwarded.codex_account_id.as_deref() {
        push_env(&mut args, "CODEX_ACCOUNT_ID", value);
    }

    args.push(sandbox.image.clone());
    args.push("bash".to_string());
    args.push("-lc".to_string());
    args.push(script);

    let output = docker_output(&args)?;
    if !output.status.success() {
        return Err(humanize_docker_error(&best_output(&output)));
    }

    let store = ShepherdSessionStore::open()
        .await
        .map_err(|error| format!("failed to open session store: {}", error))?;
    let scope_key = scope_key(scope);
    let scope_json = serde_json::to_string(scope)
        .map_err(|error| format!("failed to serialize scope json: {}", error))?;
    store
        .upsert_session(
            match scope {
                ShepherdScope::General => None,
                ShepherdScope::Shepherd { project_id, .. } => Some(*project_id),
                ShepherdScope::Thread { project_id, .. } => Some(*project_id),
                ShepherdScope::Librarian { project_id, .. } => Some(*project_id),
            },
            &scope_key,
            &scope_json,
            Some(&work_dir.display().to_string()),
            Some(env_fingerprint.as_str()),
            runtime_fingerprint.as_deref(),
            &socket_path.display().to_string(),
            Some(&container_name),
            "starting",
            None,
        )
        .await
        .map_err(|error| format!("failed to persist session record: {}", error))?;

    let startup_deadline = std::time::Instant::now() + Duration::from_secs(30);
    let startup_result = loop {
        if wait_for_worker_socket(&socket_path, Duration::from_millis(200))
            .await
            .is_ok()
        {
            break Ok(());
        }

        if std::time::Instant::now() >= startup_deadline {
            break Err("worker socket did not become ready within 30 seconds".to_string());
        }

        match container_is_running(&container_name) {
            Ok(true) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Ok(false) => {
                break Err("worker container exited before opening its RPC socket".to_string());
            }
            Err(error) => break Err(error),
        }
    };

    if let Err(error) = startup_result {
        let logs = docker_logs(&container_name);
        let message = if logs.trim().is_empty() {
            error
        } else {
            format!("{error}\n\n[hirsel] worker logs ->\n{logs}")
        };
        let _ = store.set_status(&scope_key, "failed", Some(&message)).await;
        let _ = remove_container_if_present(&container_name);
        return Err(message);
    }

    store
        .set_status(&scope_key, "idle", None)
        .await
        .map_err(|error| format!("failed to mark session idle: {}", error))?;
    store
        .get_session(&scope_key)
        .await
        .map_err(|error| format!("failed to reload session: {}", error))?
        .ok_or_else(|| "worker session disappeared after startup".to_string())
}

pub(super) async fn ensure_scope_session(
    scope: &ShepherdScope,
) -> Result<ShepherdScopeSession, String> {
    let scope_key = scope_key(scope);
    let (work_dir, sandbox) = prepare_scope_runtime(scope).await?;
    let env_fingerprint = current_env_fingerprint(&work_dir).await?;
    let runtime_fingerprint = current_worker_runtime_fingerprint(&sandbox.image)?;
    let socket_path = worker_socket_path(scope);
    let store = ShepherdSessionStore::open()
        .await
        .map_err(|error| format!("failed to open session store: {}", error))?;
    if let Some(session) = store
        .get_session(&scope_key)
        .await
        .map_err(|error| format!("failed to load session record: {}", error))?
    {
        if session.env_fingerprint.as_deref() == Some(env_fingerprint.as_str())
            && session.runtime_fingerprint == runtime_fingerprint
            && socket_path.exists()
            && wait_for_worker_socket(&socket_path, Duration::from_millis(200))
                .await
                .is_ok()
        {
            let _ = store.touch_seen(&scope_key).await;
            return Ok(session);
        }
        if let Some(container_name) = session.container_name.as_deref() {
            let _ = remove_container_if_present(container_name);
        }
    }
    start_scope_container(scope).await
}

pub(super) async fn stop_scope_session(scope: &ShepherdScope) -> Result<(), String> {
    let scope_key = scope_key(scope);
    let store = ShepherdSessionStore::open()
        .await
        .map_err(|error| format!("failed to open session store: {}", error))?;
    if let Some(session) = store
        .get_session(&scope_key)
        .await
        .map_err(|error| format!("failed to load session record: {}", error))?
    {
        if let Some(container_name) = session.container_name.as_deref() {
            remove_container_if_present(container_name)?;
        }
        if Path::new(&session.socket_path).exists() {
            let _ = std::fs::remove_file(&session.socket_path);
        }
        store
            .delete_session(&scope_key)
            .await
            .map_err(|error| format!("failed to delete session record: {}", error))?;
    }
    Ok(())
}

pub(super) fn current_server_control_socket_path() -> PathBuf {
    server_control_socket_path()
}
