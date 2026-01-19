//! HTTP file receiver for remote workers.
//!
//! This module implements a lightweight HTTP server that receives file uploads
//! from the coordinator. This is used by sprite workers to receive the project
//! files (spec, code, etc.) without needing to pull from a URL.
//!
//! Endpoints:
//! - GET /health - Health check, returns 200 OK when ready
//! - POST /upload - Receive tarball and extract to work directory

use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use flate2::read::GzDecoder;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tar::Archive;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tracing::{error, info};

/// Default port for the file receiver
pub const FILE_RECEIVER_PORT: u16 = 19800;

/// Maximum upload size (500MB)
const MAX_UPLOAD_SIZE: usize = 500 * 1024 * 1024;

/// Shared state for the file server
struct FileServerState {
    work_dir: PathBuf,
    received: AtomicBool,
    shutdown_tx: tokio::sync::Mutex<Option<oneshot::Sender<()>>>,
    files_tx: tokio::sync::Mutex<Option<oneshot::Sender<Result<(), String>>>>,
}

/// Result of starting the file server
pub struct FileServerHandle {
    /// Port the server is listening on
    pub port: u16,
    /// Channel to receive notification when files are uploaded
    pub files_ready: oneshot::Receiver<Result<(), String>>,
    /// Server task handle
    pub task: tokio::task::JoinHandle<()>,
}

/// Start the file receiver server.
///
/// Returns a handle with the port and a channel that signals when files are ready.
pub async fn start_file_server(
    work_dir: PathBuf,
    port: Option<u16>,
) -> Result<FileServerHandle, Box<dyn std::error::Error + Send + Sync>> {
    let port = port.unwrap_or(FILE_RECEIVER_PORT);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let (files_tx, files_rx) = oneshot::channel::<Result<(), String>>();

    let state = Arc::new(FileServerState {
        work_dir,
        received: AtomicBool::new(false),
        shutdown_tx: tokio::sync::Mutex::new(Some(shutdown_tx)),
        files_tx: tokio::sync::Mutex::new(Some(files_tx)),
    });

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/upload", post(upload_handler))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_SIZE))
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    let actual_port = listener.local_addr()?.port();

    info!("File receiver listening on port {}", actual_port);

    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });

    Ok(FileServerHandle {
        port: actual_port,
        files_ready: files_rx,
        task,
    })
}

/// Health check endpoint
async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "ready")
}

/// Upload endpoint - receives tarball and extracts it
async fn upload_handler(
    State(state): State<Arc<FileServerState>>,
    body: axum::body::Bytes,
) -> (StatusCode, &'static str) {
    // Prevent multiple uploads
    if state.received.swap(true, Ordering::SeqCst) {
        return (StatusCode::CONFLICT, "Files already uploaded");
    }

    info!("Received {} bytes of data", body.len());

    // Extract tarball
    let result = extract_tarball(&body, &state.work_dir);

    let success = match &result {
        Ok(()) => {
            info!("Successfully extracted files to {:?}", state.work_dir);
            true
        }
        Err(e) => {
            error!("Failed to extract tarball: {}", e);
            false
        }
    };

    // Signal that files are ready (spawn to avoid blocking)
    let state_for_signal = Arc::clone(&state);
    tokio::spawn(async move {
        if let Some(tx) = state_for_signal.files_tx.lock().await.take() {
            let send_result = if success {
                Ok(())
            } else {
                Err("Failed to extract tarball".to_string())
            };
            let _ = tx.send(send_result);
        }

        // Schedule shutdown after signaling
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        if let Some(tx) = state_for_signal.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
        }
    });

    match result {
        Ok(()) => (StatusCode::OK, "Files extracted successfully"),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Failed to extract files"),
    }
}

/// Extract a gzipped tarball to the work directory
fn extract_tarball(data: &[u8], work_dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    // Create work directory if it doesn't exist
    std::fs::create_dir_all(work_dir)?;

    // Try to decompress as gzip first
    let decoder = GzDecoder::new(data);
    let mut archive = Archive::new(decoder);

    // Extract all files
    archive.unpack(work_dir)?;

    Ok(())
}

/// Extract a plain tarball (no compression)
#[allow(dead_code)]
fn extract_tar(data: &[u8], work_dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(work_dir)?;

    let mut archive = Archive::new(data);
    archive.unpack(work_dir)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_file_server_starts() {
        let temp_dir = tempfile::tempdir().unwrap();
        let handle = start_file_server(temp_dir.path().to_path_buf(), Some(0))
            .await
            .unwrap();

        // Server should be listening
        assert!(handle.port > 0);

        // Health check should work
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://127.0.0.1:{}/health", handle.port))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        handle.task.abort();
    }

    #[tokio::test]
    async fn test_file_upload() {
        let temp_dir = tempfile::tempdir().unwrap();
        let handle = start_file_server(temp_dir.path().to_path_buf(), Some(0))
            .await
            .unwrap();

        // Create a simple tarball in memory
        let mut tar_data = Vec::new();
        {
            let gz_encoder =
                flate2::write::GzEncoder::new(&mut tar_data, flate2::Compression::default());
            let mut tar_builder = tar::Builder::new(gz_encoder);

            // Add a test file
            let mut header = tar::Header::new_gnu();
            let content = b"test content";
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar_builder
                .append_data(&mut header, "test.txt", &content[..])
                .unwrap();

            tar_builder.finish().unwrap();
        }

        // Upload the tarball
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{}/upload", handle.port))
            .body(tar_data)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        // Wait for files_ready signal
        let result = handle.files_ready.await.unwrap();
        assert!(result.is_ok());

        // Check file was extracted
        let extracted_path = temp_dir.path().join("test.txt");
        assert!(extracted_path.exists());
        let content = std::fs::read_to_string(extracted_path).unwrap();
        assert_eq!(content, "test content");
    }
}
