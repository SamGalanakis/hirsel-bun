use std::path::PathBuf;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::backend::{ProjectStore, ShepherdThreadStore};

#[derive(Deserialize)]
pub struct TerminalQuery {
    scope: String,
}

#[derive(Debug, Clone)]
enum TerminalScopeTarget {
    Host,
    Shepherd,
    Thread(String),
}

#[derive(Debug)]
enum TerminalEvent {
    Output(String),
    Exit(i32),
    Error(String),
}

enum ClientTerminalMessage {
    Input(String),
    Resize,
}

struct TerminalProcess {
    stdin_tx: UnboundedSender<Vec<u8>>,
    /// Handle kept alive so the stdin writer task can detect shutdown.
    _child_handle: tokio::task::JoinHandle<()>,
}

pub async fn project_terminal(
    Path(project_id): Path<i64>,
    Query(query): Query<TerminalQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, (StatusCode, String)> {
    let target = parse_scope(&query.scope)?;
    Ok(ws.on_upgrade(move |socket| handle_terminal_socket(socket, project_id, target)))
}

async fn handle_terminal_socket(
    mut socket: WebSocket,
    project_id: i64,
    target: TerminalScopeTarget,
) {
    let (process, mut output_rx) = match spawn_terminal_process(project_id, &target).await {
        Ok(value) => value,
        Err(error) => {
            let _ = send_json(&mut socket, json!({ "type": "error", "message": error })).await;
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };

    loop {
        tokio::select! {
            maybe_msg = socket.recv() => {
                match maybe_msg {
                    Some(Ok(Message::Text(text))) => {
                        match parse_client_message(&text) {
                            Ok(ClientTerminalMessage::Input(data)) => {
                                if process.stdin_tx.send(data.into_bytes()).is_err() {
                                    let _ = send_json(&mut socket, json!({ "type": "error", "message": "terminal stdin is closed" })).await;
                                    break;
                                }
                            }
                            Ok(ClientTerminalMessage::Resize) => {
                                // Resize is not supported without a PTY — silently accept.
                            }
                            Err(error) => {
                                let _ = send_json(&mut socket, json!({ "type": "error", "message": error })).await;
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Binary(_))) => {}
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) => break,
                    Some(Err(error)) => {
                        tracing::debug!(%error, project_id, "terminal websocket receive error");
                        break;
                    }
                    None => break,
                }
            }
            maybe_event = output_rx.recv() => {
                match maybe_event {
                    Some(TerminalEvent::Output(data)) => {
                        if send_json(&mut socket, json!({ "type": "output", "data": data })).await.is_err() {
                            break;
                        }
                    }
                    Some(TerminalEvent::Exit(exit_code)) => {
                        let _ = send_json(&mut socket, json!({ "type": "exit", "exitCode": exit_code })).await;
                        break;
                    }
                    Some(TerminalEvent::Error(message)) => {
                        let _ = send_json(&mut socket, json!({ "type": "error", "message": message })).await;
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    drop(process);
    let _ = socket.send(Message::Close(None)).await;
}

fn parse_scope(value: &str) -> Result<TerminalScopeTarget, (StatusCode, String)> {
    let trimmed = value.trim();
    if trimmed == "host" {
        return Ok(TerminalScopeTarget::Host);
    }
    if trimmed == "shepherd" {
        return Ok(TerminalScopeTarget::Shepherd);
    }
    if let Some(thread_id) = trimmed.strip_prefix("thread:") {
        let thread_id = thread_id.trim();
        if !thread_id.is_empty() {
            return Ok(TerminalScopeTarget::Thread(thread_id.to_string()));
        }
    }
    Err((
        StatusCode::BAD_REQUEST,
        format!("Invalid terminal scope '{trimmed}'"),
    ))
}

async fn resolve_project_cwd(project_id: i64) -> Result<PathBuf, String> {
    let store = ProjectStore::open()
        .await
        .map_err(|error| format!("failed to open project store: {}", error))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|error| format!("failed to load project {}: {}", project_id, error))?;
    if let Some(cwd) = project
        .shepherd_cwd
        .as_deref()
        .filter(|p| !p.trim().is_empty())
    {
        let path = PathBuf::from(cwd);
        if path.is_dir() {
            return Ok(path);
        }
    }
    for ws in &project.workspaces {
        if let Some(path) = ws.path.as_deref().filter(|p| !p.trim().is_empty()) {
            let path = PathBuf::from(path);
            if path.is_dir() {
                return Ok(path);
            }
        }
    }
    Ok(dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")))
}

async fn spawn_terminal_process(
    project_id: i64,
    target: &TerminalScopeTarget,
) -> Result<(TerminalProcess, UnboundedReceiver<TerminalEvent>), String> {
    match target {
        TerminalScopeTarget::Host | TerminalScopeTarget::Shepherd => {
            let workdir = resolve_project_cwd(project_id).await?;
            spawn_shell_process(build_host_command(workdir)).await
        }
        TerminalScopeTarget::Thread(thread_id) => {
            let thread_store = ShepherdThreadStore::open()
                .await
                .map_err(|error| format!("failed to open shepherd thread store: {}", error))?;
            let thread = thread_store
                .get_thread(thread_id)
                .await
                .map_err(|error| format!("failed to load thread {}: {}", thread_id, error))?;
            if thread.project_id != project_id {
                return Err(format!(
                    "thread {} does not belong to project {}",
                    thread_id, project_id
                ));
            }
            let workdir = match thread.cwd.as_deref().filter(|p| !p.trim().is_empty()) {
                Some(cwd) => PathBuf::from(cwd),
                None => resolve_project_cwd(project_id).await?,
            };
            spawn_shell_process(build_host_command(workdir)).await
        }
    }
}

fn build_host_command(workdir: PathBuf) -> Command {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    let mut cmd = Command::new(&shell);
    if shell_supports_login(&shell) {
        cmd.arg("-l");
    }
    cmd.current_dir(workdir);
    cmd.env("TERM", "xterm-256color");
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd
}

async fn spawn_shell_process(
    mut cmd: Command,
) -> Result<(TerminalProcess, UnboundedReceiver<TerminalEvent>), String> {
    let mut child = cmd
        .spawn()
        .map_err(|error| format!("failed to start terminal: {}", error))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "failed to capture terminal stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "failed to capture terminal stderr".to_string())?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "failed to capture terminal stdin".to_string())?;

    let (event_tx, event_rx) = unbounded_channel();
    let (stdin_tx, stdin_rx) = unbounded_channel::<Vec<u8>>();

    // Spawn a task that forwards stdin_rx bytes into the child's stdin.
    let stdin_handle = tokio::spawn(drive_stdin(stdin, stdin_rx));

    // Spawn tasks that read stdout and stderr and send TerminalEvent::Output.
    tokio::spawn(drive_output_reader(stdout, event_tx.clone()));
    tokio::spawn(drive_output_reader(stderr, event_tx.clone()));

    // Spawn a task that waits for the child to exit.
    tokio::spawn(drive_child_wait(child, event_tx));

    Ok((
        TerminalProcess {
            stdin_tx,
            _child_handle: stdin_handle,
        },
        event_rx,
    ))
}

async fn drive_stdin(mut stdin: tokio::process::ChildStdin, mut rx: UnboundedReceiver<Vec<u8>>) {
    while let Some(data) = rx.recv().await {
        if stdin.write_all(&data).await.is_err() {
            break;
        }
        if stdin.flush().await.is_err() {
            break;
        }
    }
    // Dropping stdin signals EOF to the child.
}

async fn drive_output_reader<R: AsyncReadExt + Unpin>(
    mut reader: R,
    tx: UnboundedSender<TerminalEvent>,
) {
    let mut buffer = [0u8; 4096];
    let mut pending = String::new();
    let mut last_flush = Instant::now();

    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(n) => {
                pending.push_str(&String::from_utf8_lossy(&buffer[..n]));
                let should_flush = pending.len() >= 16_384
                    || pending.contains('\n')
                    || last_flush.elapsed() >= Duration::from_millis(16);
                if !should_flush {
                    continue;
                }
                if tx
                    .send(TerminalEvent::Output(std::mem::take(&mut pending)))
                    .is_err()
                {
                    break;
                }
                last_flush = Instant::now();
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                if !pending.is_empty() {
                    let _ = tx.send(TerminalEvent::Output(std::mem::take(&mut pending)));
                }
                let _ = tx.send(TerminalEvent::Error(format!(
                    "terminal read failed: {}",
                    error
                )));
                break;
            }
        }
    }

    if !pending.is_empty() {
        let _ = tx.send(TerminalEvent::Output(pending));
    }
}

async fn drive_child_wait(mut child: Child, tx: UnboundedSender<TerminalEvent>) {
    let exit_code = match child.wait().await {
        Ok(status) => status.code().unwrap_or(-1),
        Err(_) => -1,
    };
    let _ = tx.send(TerminalEvent::Exit(exit_code));
}

fn parse_client_message(payload: &str) -> Result<ClientTerminalMessage, String> {
    let value: serde_json::Value = serde_json::from_str(payload)
        .map_err(|error| format!("Invalid terminal message: {}", error))?;
    match value.get("type").and_then(|value| value.as_str()) {
        Some("input") => {
            let data = value
                .get("data")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            Ok(ClientTerminalMessage::Input(data))
        }
        Some("resize") => {
            let _cols = parse_terminal_dimension(value.get("cols"), "cols")?;
            let _rows = parse_terminal_dimension(value.get("rows"), "rows")?;
            Ok(ClientTerminalMessage::Resize)
        }
        Some(other) => Err(format!("Unsupported terminal message type '{}'", other)),
        None => Err("Terminal message missing type".to_string()),
    }
}

fn parse_terminal_dimension(value: Option<&serde_json::Value>, name: &str) -> Result<u16, String> {
    let raw = value
        .and_then(|value| value.as_u64())
        .ok_or_else(|| format!("Terminal message missing {}", name))?;
    u16::try_from(raw)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("Invalid terminal {} {}", name, raw))
}

async fn send_json(socket: &mut WebSocket, value: serde_json::Value) -> Result<(), axum::Error> {
    socket.send(Message::Text(value.to_string().into())).await
}

fn shell_supports_login(shell_path: &str) -> bool {
    matches!(
        shell_path.rsplit('/').next().unwrap_or(shell_path),
        "bash" | "zsh" | "ksh" | "mksh" | "fish"
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_scope, TerminalScopeTarget};

    #[test]
    fn parses_supported_scopes() {
        assert!(matches!(
            parse_scope("host").unwrap(),
            TerminalScopeTarget::Host
        ));
        assert!(matches!(
            parse_scope("shepherd").unwrap(),
            TerminalScopeTarget::Shepherd
        ));
        assert!(matches!(
            parse_scope("thread:abc").unwrap(),
            TerminalScopeTarget::Thread(thread_id) if thread_id == "abc"
        ));
    }
}
