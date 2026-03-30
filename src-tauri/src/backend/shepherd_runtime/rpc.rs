use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkerRequest {
    Ping,
    Status,
    Interrupt,
    RunTurn {
        user_chunks: Vec<ShepherdMessageChunk>,
        #[serde(default)]
        focus: Option<ShepherdTaskFocus>,
        #[serde(default)]
        user_message_id: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkerReply {
    Pong,
    Status {
        status: String,
    },
    Accepted,
    Event {
        event: WorkerStreamEvent,
    },
    Finished {
        assistant_chunks: Vec<ShepherdMessageChunk>,
        state_json: String,
        summary: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkerStreamEvent {
    TextDelta {
        content: String,
    },
    Tool {
        id: String,
        title: String,
        #[serde(default)]
        kind: Option<String>,
        status: String,
        #[serde(default)]
        input: Option<String>,
        #[serde(default)]
        output: Option<String>,
    },
    Message {
        text: String,
        kind: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerControlRequest {
    DispatchScopeMessage {
        scope: ShepherdScope,
        user_chunks: Vec<ShepherdMessageChunk>,
        #[serde(default)]
        focus: Option<ShepherdTaskFocus>,
    },
    CreateThread {
        project_id: i64,
        title: String,
        objective: String,
        summary: String,
    },
    InterruptScopeTurn {
        scope: ShepherdScope,
    },
    StopScopeSession {
        scope: ShepherdScope,
    },
    ArchiveThread {
        project_id: i64,
        thread_id: String,
    },
    DeleteThread {
        project_id: i64,
        thread_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerControlReply {
    Ok {
        #[serde(default)]
        payload: Option<Value>,
    },
    Error {
        message: String,
    },
}

pub fn server_control_socket_path() -> PathBuf {
    crate::backend::config::hirsel_dir()
        .join("server")
        .join("control.sock")
}

pub async fn write_json_line<W, T>(writer: &mut W, value: &T) -> Result<(), String>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let mut bytes = serde_json::to_vec(value)
        .map_err(|error| format!("failed to serialize rpc payload: {}", error))?;
    bytes.push(b'\n');
    writer
        .write_all(&bytes)
        .await
        .map_err(|error| format!("failed to write rpc payload: {}", error))?;
    writer
        .flush()
        .await
        .map_err(|error| format!("failed to flush rpc payload: {}", error))
}

pub async fn read_json_line<R, T>(reader: &mut BufReader<R>) -> Result<T, String>
where
    R: tokio::io::AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .await
        .map_err(|error| format!("failed to read rpc payload: {}", error))?;
    if bytes == 0 {
        return Err("rpc connection closed".to_string());
    }
    serde_json::from_str(line.trim_end())
        .map_err(|error| format!("failed to decode rpc payload: {}", error))
}

pub async fn connect_worker_socket(socket_path: &Path) -> Result<UnixStream, String> {
    UnixStream::connect(socket_path)
        .await
        .map_err(|error| format!("failed to connect worker socket: {}", error))
}

pub async fn wait_for_worker_socket(socket_path: &Path, timeout: Duration) -> Result<(), String> {
    let started = std::time::Instant::now();
    loop {
        if socket_path.exists() && connect_worker_socket(socket_path).await.is_ok() {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            return Err(format!(
                "worker socket did not become ready: {}",
                socket_path.display()
            ));
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

pub async fn send_server_control_request(
    request: &ServerControlRequest,
) -> Result<Option<Value>, String> {
    let socket_path = std::env::var("HIRSEL_SERVER_RPC_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| server_control_socket_path());
    let stream = UnixStream::connect(&socket_path)
        .await
        .map_err(|error| format!("failed to connect server control socket: {}", error))?;
    let (read_half, mut write_half) = stream.into_split();
    write_json_line(&mut write_half, request).await?;
    let mut reader = BufReader::new(read_half);
    match read_json_line::<_, ServerControlReply>(&mut reader).await? {
        ServerControlReply::Ok { payload } => Ok(payload),
        ServerControlReply::Error { message } => Err(message),
    }
}
