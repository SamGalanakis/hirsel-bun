use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::response::Response;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::backend::shepherd_runtime::{ensure_scope_session_ready, ShepherdScope};
use crate::backend::{ensure_project_workspace, ShepherdThreadStore};

const DEFAULT_PTY_SIZE: PtySize = PtySize {
    rows: 24,
    cols: 80,
    pixel_width: 0,
    pixel_height: 0,
};

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
    Resize { cols: u16, rows: u16 },
}

struct TerminalProcess {
    _master: Box<dyn MasterPty + Send>,
    writer: Arc<StdMutex<Option<Box<dyn Write + Send>>>>,
    killer: Arc<StdMutex<Box<dyn ChildKiller + Send + Sync>>>,
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
    let (mut process, mut output_rx) = match spawn_terminal_process(project_id, &target).await {
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
                                let writer = Arc::clone(&process.writer);
                                if let Err(error) = write_terminal_input(writer, data).await {
                                    let _ = send_json(&mut socket, json!({ "type": "error", "message": error })).await;
                                    break;
                                }
                            }
                            Ok(ClientTerminalMessage::Resize { cols, rows }) => {
                                if let Err(error) = process.resize(cols, rows) {
                                    let _ = send_json(&mut socket, json!({ "type": "error", "message": error })).await;
                                    break;
                                }
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

    process.shutdown();
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

async fn spawn_terminal_process(
    project_id: i64,
    target: &TerminalScopeTarget,
) -> Result<(TerminalProcess, UnboundedReceiver<TerminalEvent>), String> {
    match target {
        TerminalScopeTarget::Host => {
            let workspace = ensure_project_workspace(project_id).await?;
            spawn_host_shell(workspace.central_dir)
        }
        TerminalScopeTarget::Shepherd => {
            let session = ensure_scope_session_ready(&ShepherdScope::Shepherd {
                project_id,
                workspace_path: None,
                focus: None,
            })
            .await?;
            let container_name = session
                .container_name
                .ok_or_else(|| "Shepherd runtime did not expose a container name".to_string())?;
            spawn_container_shell(container_name)
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

            let session = ensure_scope_session_ready(&ShepherdScope::Thread {
                project_id,
                thread_id: thread.id,
                title: thread.title,
                workspace_path: None,
                focus: None,
            })
            .await?;
            let container_name = session
                .container_name
                .ok_or_else(|| "Thread runtime did not expose a container name".to_string())?;
            spawn_container_shell(container_name)
        }
    }
}

fn spawn_host_shell(
    workdir: PathBuf,
) -> Result<(TerminalProcess, UnboundedReceiver<TerminalEvent>), String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    let mut cmd = CommandBuilder::new(&shell);
    if shell_supports_login(&shell) {
        cmd.arg("-l");
    }
    cmd.cwd(workdir);
    spawn_pty_command(cmd)
}

fn spawn_container_shell(
    container_name: String,
) -> Result<(TerminalProcess, UnboundedReceiver<TerminalEvent>), String> {
    let mut cmd = CommandBuilder::new("docker");
    cmd.arg("exec");
    cmd.arg("-i");
    cmd.arg("-t");
    cmd.arg("-w");
    cmd.arg("/work");
    cmd.arg("-e");
    cmd.arg("TERM=xterm-256color");
    cmd.arg("-e");
    cmd.arg("COLORTERM=truecolor");
    cmd.arg(container_name);
    cmd.arg("bash");
    cmd.arg("-l");
    spawn_pty_command(cmd)
}

fn spawn_pty_command(
    cmd: CommandBuilder,
) -> Result<(TerminalProcess, UnboundedReceiver<TerminalEvent>), String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(DEFAULT_PTY_SIZE)
        .map_err(|error| format!("failed to open PTY: {}", error))?;

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|error| format!("failed to start terminal: {}", error))?;
    let killer = child.clone_killer();
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| format!("failed to open terminal reader: {}", error))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| format!("failed to open terminal writer: {}", error))?;
    drop(pair.slave);

    let (tx, rx) = unbounded_channel();
    spawn_reader_thread(reader, tx.clone());
    spawn_wait_thread(child, tx);

    Ok((
        TerminalProcess {
            _master: pair.master,
            writer: Arc::new(StdMutex::new(Some(writer))),
            killer: Arc::new(StdMutex::new(killer)),
        },
        rx,
    ))
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
            let cols = parse_terminal_dimension(value.get("cols"), "cols")?;
            let rows = parse_terminal_dimension(value.get("rows"), "rows")?;
            Ok(ClientTerminalMessage::Resize { cols, rows })
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

impl TerminalProcess {
    fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self._master
            .resize(PtySize {
                cols,
                rows,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("failed to resize terminal: {}", error))
    }

    fn shutdown(&mut self) {
        if let Ok(mut writer) = self.writer.lock() {
            writer.take();
        }
        if let Ok(mut killer) = self.killer.lock() {
            let _ = killer.kill();
        }
    }
}

async fn write_terminal_input(
    writer: Arc<StdMutex<Option<Box<dyn Write + Send>>>>,
    data: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let mut guard = writer
            .lock()
            .map_err(|_| "terminal writer lock poisoned".to_string())?;
        let writer = guard
            .as_mut()
            .ok_or_else(|| "terminal stdin is closed".to_string())?;
        writer
            .write_all(data.as_bytes())
            .map_err(|error| format!("failed to write terminal input: {}", error))?;
        writer
            .flush()
            .map_err(|error| format!("failed to flush terminal input: {}", error))
    })
    .await
    .map_err(|error| format!("terminal write task failed: {}", error))?
}

fn spawn_reader_thread(
    mut reader: Box<dyn std::io::Read + Send>,
    tx: UnboundedSender<TerminalEvent>,
) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        let mut pending = String::new();
        let mut last_flush = Instant::now();
        loop {
            match reader.read(&mut buffer) {
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
    });
}

fn spawn_wait_thread(
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
    tx: UnboundedSender<TerminalEvent>,
) {
    std::thread::spawn(move || {
        let exit_code = child
            .wait()
            .map(|status| i32::try_from(status.exit_code()).unwrap_or(i32::MAX))
            .unwrap_or(-1);
        let _ = tx.send(TerminalEvent::Exit(exit_code));
    });
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
