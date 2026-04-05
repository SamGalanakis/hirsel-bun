use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
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
    ExecShell {
        args: Value,
    },
    WriteShell {
        args: Value,
    },
    ProxyHttp {
        port: u16,
        protocol: String,
        request: ProxyHttpRequest,
    },
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
    ToolResult {
        success: bool,
        result: Value,
    },
    ProxyHttpResponse(ProxyHttpResponse),
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
#[serde(rename_all = "camelCase")]
pub struct ProxyHttpRequest {
    pub method: String,
    pub path_and_query: String,
    pub headers: Vec<(String, String)>,
    pub body_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyHttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewForwardInfo {
    pub id: String,
    pub project_id: i64,
    pub thread_id: String,
    pub protocol: String,
    pub port: u16,
    pub host_port: u16,
    pub user_url: String,
    pub shepherd_url: String,
    pub label: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerToolResultPayload {
    pub success: bool,
    pub result: Value,
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
    ExecThreadShell {
        project_id: i64,
        thread_id: String,
        args: Value,
    },
    WriteThreadShell {
        project_id: i64,
        thread_id: String,
        args: Value,
    },
    ArchiveThread {
        project_id: i64,
        thread_id: String,
    },
    PromoteThread {
        project_id: i64,
        thread_id: String,
    },
    ForwardThreadPort {
        project_id: i64,
        thread_id: String,
        port: u16,
        protocol: String,
        label: String,
    },
    ListThreadPortForwards {
        project_id: i64,
        #[serde(default)]
        thread_id: Option<String>,
    },
    ClosePortForward {
        forward_id: String,
    },
    DeleteThread {
        project_id: i64,
        thread_id: String,
    },
    LoadScopeMessages {
        scope: ShepherdScope,
        limit: usize,
        #[serde(default)]
        skip_message_id: Option<i64>,
    },
    LoadScopeState {
        scope: ShepherdScope,
    },
    ExecuteShepherdTool {
        project_id: i64,
        name: String,
        args: Value,
    },
    ExecuteLibrarianTool {
        project_id: i64,
        name: String,
        args: Value,
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

pub fn encode_http_body(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn decode_http_body(value: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|error| format!("failed to decode proxied body: {}", error))
}
