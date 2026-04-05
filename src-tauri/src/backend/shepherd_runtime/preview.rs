use std::collections::HashMap;
use std::sync::OnceLock;

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{Response, StatusCode};
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use super::rpc::{
    connect_worker_socket, decode_http_body, encode_http_body, read_json_line, write_json_line,
    PreviewForwardInfo, ProxyHttpRequest, WorkerReply, WorkerRequest,
};
use super::sandbox::ensure_scope_session;
use super::types::ShepherdScope;
use crate::backend::db::utc_now;
use crate::backend::{ShepherdThread, ShepherdThreadStore};

struct PreviewForwardHandle {
    info: PreviewForwardInfo,
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
struct PreviewListenerState {
    info: PreviewForwardInfo,
}

static PREVIEW_FORWARDS: OnceLock<RwLock<HashMap<String, PreviewForwardHandle>>> = OnceLock::new();

fn preview_forwards() -> &'static RwLock<HashMap<String, PreviewForwardHandle>> {
    PREVIEW_FORWARDS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn thread_scope(thread: &ShepherdThread) -> ShepherdScope {
    ShepherdScope::Thread {
        project_id: thread.project_id,
        thread_id: thread.id.clone(),
        title: thread.title.clone(),
        workspace_path: thread.workspace_path.clone(),
        focus: None,
    }
}

async fn load_thread(project_id: i64, thread_id: &str) -> Result<ShepherdThread, String> {
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|error| format!("failed to open shepherd thread store: {}", error))?;
    let thread = store
        .get_thread(thread_id)
        .await
        .map_err(|error| format!("failed to load thread {}: {}", thread_id, error))?;
    if thread.project_id != project_id {
        return Err(format!(
            "thread {} does not belong to project {}",
            thread_id, project_id
        ));
    }
    Ok(thread)
}

async fn proxy_thread_http(
    info: &PreviewForwardInfo,
    request: ProxyHttpRequest,
) -> Result<super::rpc::ProxyHttpResponse, String> {
    let thread = load_thread(info.project_id, &info.thread_id).await?;
    let session = ensure_scope_session(&thread_scope(&thread)).await?;
    let stream = connect_worker_socket(std::path::Path::new(&session.socket_path)).await?;
    let (read_half, mut write_half) = stream.into_split();
    write_json_line(
        &mut write_half,
        &WorkerRequest::ProxyHttp {
            port: info.port,
            protocol: info.protocol.clone(),
            request,
        },
    )
    .await?;
    let mut reader = tokio::io::BufReader::new(read_half);
    match read_json_line::<_, WorkerReply>(&mut reader).await? {
        WorkerReply::ProxyHttpResponse(response) => Ok(response),
        WorkerReply::Error { message } => Err(message),
        other => Err(format!(
            "unexpected worker reply during preview proxy: {:?}",
            other
        )),
    }
}

fn text_response(status: StatusCode, message: impl Into<String>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Body::from(message.into()))
        .unwrap_or_else(|_| Response::new(Body::from("preview proxy error")))
}

async fn handle_preview_request(
    State(state): State<PreviewListenerState>,
    request: Request<Body>,
) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    let body = match to_bytes(body, usize::MAX).await {
        Ok(body) => body,
        Err(error) => {
            return text_response(
                StatusCode::BAD_REQUEST,
                format!("failed to read preview request body: {}", error),
            );
        }
    };
    let request = ProxyHttpRequest {
        method: parts.method.as_str().to_string(),
        path_and_query,
        headers: parts
            .headers
            .iter()
            .filter_map(|(name, value)| {
                let value = value.to_str().ok()?.to_string();
                Some((name.as_str().to_string(), value))
            })
            .collect(),
        body_base64: encode_http_body(&body),
    };

    match proxy_thread_http(&state.info, request).await {
        Ok(response) => {
            let body = match decode_http_body(&response.body_base64) {
                Ok(body) => body,
                Err(error) => {
                    return text_response(StatusCode::BAD_GATEWAY, error);
                }
            };
            let mut builder = Response::builder().status(response.status);
            for (name, value) in response.headers {
                builder = builder.header(&name, &value);
            }
            builder.body(Body::from(body)).unwrap_or_else(|error| {
                text_response(
                    StatusCode::BAD_GATEWAY,
                    format!("failed to build preview response: {}", error),
                )
            })
        }
        Err(error) => text_response(StatusCode::BAD_GATEWAY, error),
    }
}

async fn spawn_preview_listener(info: PreviewForwardInfo) -> Result<PreviewForwardHandle, String> {
    let listener = TcpListener::bind(("0.0.0.0", 0))
        .await
        .map_err(|error| format!("failed to bind preview listener: {}", error))?;
    let host_port = listener
        .local_addr()
        .map_err(|error| format!("failed to inspect preview listener: {}", error))?
        .port();
    let mut info = info;
    info.host_port = host_port;
    info.user_url = format!("http://127.0.0.1:{}/", host_port);
    info.shepherd_url = format!("http://host.docker.internal:{}/", host_port);
    let cancel = CancellationToken::new();
    let state = PreviewListenerState { info: info.clone() };
    let router = Router::new()
        .fallback(handle_preview_request)
        .with_state(state);
    let shutdown = cancel.clone();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                shutdown.cancelled().await;
            })
            .await;
    });
    Ok(PreviewForwardHandle { info, cancel, task })
}

pub(super) async fn open_preview_forward(
    project_id: i64,
    thread_id: &str,
    port: u16,
    protocol: &str,
    label: &str,
) -> Result<PreviewForwardInfo, String> {
    if !matches!(protocol, "http" | "https") {
        return Err(format!(
            "unsupported preview protocol '{}'; use http or https",
            protocol
        ));
    }
    let label = label.trim();
    if label.is_empty() {
        return Err("preview label is required".to_string());
    }
    let _ = load_thread(project_id, thread_id).await?;

    {
        let mut forwards = preview_forwards().write().await;
        if let Some(existing) = forwards.values_mut().find(|forward| {
            forward.info.project_id == project_id
                && forward.info.thread_id == thread_id
                && forward.info.port == port
                && forward.info.protocol == protocol
        }) {
            if existing.info.label != label {
                existing.info.label = label.to_string();
            }
            return Ok(existing.info.clone());
        }
    }

    let id = format!("preview-{}", uuid::Uuid::new_v4().simple());
    let handle = spawn_preview_listener(PreviewForwardInfo {
        id: id.clone(),
        project_id,
        thread_id: thread_id.to_string(),
        protocol: protocol.to_string(),
        port,
        host_port: 0,
        user_url: String::new(),
        shepherd_url: String::new(),
        label: label.to_string(),
        created_at: utc_now(),
    })
    .await?;
    let info = handle.info.clone();
    preview_forwards().write().await.insert(id, handle);
    Ok(info)
}

pub(super) async fn list_preview_forwards(
    project_id: i64,
    thread_id: Option<&str>,
) -> Vec<PreviewForwardInfo> {
    let forwards = preview_forwards().read().await;
    let mut items = forwards
        .values()
        .filter(|forward| {
            forward.info.project_id == project_id
                && thread_id
                    .map(|thread_id| forward.info.thread_id == thread_id)
                    .unwrap_or(true)
        })
        .map(|forward| forward.info.clone())
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    items
}

pub(super) async fn close_preview_forward(
    forward_id: &str,
) -> Result<Option<PreviewForwardInfo>, String> {
    let handle = preview_forwards().write().await.remove(forward_id);
    let Some(handle) = handle else {
        return Ok(None);
    };
    handle.cancel.cancel();
    handle.task.abort();
    Ok(Some(handle.info))
}

pub(super) async fn close_thread_preview_forwards(project_id: i64, thread_id: &str) {
    let ids = {
        let forwards = preview_forwards().read().await;
        forwards
            .values()
            .filter(|forward| {
                forward.info.project_id == project_id && forward.info.thread_id == thread_id
            })
            .map(|forward| forward.info.id.clone())
            .collect::<Vec<_>>()
    };
    for id in ids {
        let _ = close_preview_forward(&id).await;
    }
}
