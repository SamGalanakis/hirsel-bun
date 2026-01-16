//! Git Smart HTTP server using git-http-backend CGI.
//!
//! Provides git protocol over HTTP for remote workers to clone/fetch
//! from the coordinator's staging repo.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

// =============================================================================
// Shared State
// =============================================================================

pub struct GitState {
    pub repo_path: PathBuf,
}

// =============================================================================
// Query Parameters
// =============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct ServiceQuery {
    pub service: Option<String>,
}

// =============================================================================
// Router
// =============================================================================

/// Create an axum router for git HTTP endpoints.
///
/// The router handles git smart HTTP protocol using git-http-backend CGI.
pub fn create_git_router(repo_path: PathBuf) -> Router {
    let state = Arc::new(GitState { repo_path });

    Router::new()
        .route("/", any(git_root_handler))
        .route("/*path", any(git_handler))
        .with_state(state)
}

// =============================================================================
// Handlers
// =============================================================================

async fn git_root_handler(
    State(state): State<Arc<GitState>>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ServiceQuery>,
    body: Body,
) -> impl IntoResponse {
    handle_git_request(state, method, headers, query, "", body).await
}

async fn git_handler(
    State(state): State<Arc<GitState>>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ServiceQuery>,
    Path(path): Path<String>,
    body: Body,
) -> impl IntoResponse {
    handle_git_request(state, method, headers, query, &path, body).await
}

async fn handle_git_request(
    state: Arc<GitState>,
    method: Method,
    headers: HeaderMap,
    query: ServiceQuery,
    path: &str,
    body: Body,
) -> Response {
    // Build query string from service parameter
    let query_string = query
        .service
        .as_ref()
        .map(|s| format!("service={}", s))
        .unwrap_or_default();

    // Ensure path starts with /
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };

    tracing::debug!("Git HTTP: {} {}?{}", method, path, query_string);

    // Read request body
    let body_bytes = match axum::body::to_bytes(body, 50 * 1024 * 1024).await {
        Ok(bytes) => bytes.to_vec(),
        Err(e) => {
            tracing::error!("Failed to read request body: {}", e);
            return (StatusCode::BAD_REQUEST, "Failed to read body").into_response();
        }
    };

    // Run git-http-backend CGI
    match run_git_cgi(
        &state.repo_path,
        &method,
        &path,
        &query_string,
        &headers,
        &body_bytes,
    )
    .await
    {
        Ok((status, response_headers, response_body)) => {
            let mut response = Response::builder().status(status);

            for (key, value) in response_headers {
                response = response.header(key, value);
            }

            response.body(Body::from(response_body)).unwrap()
        }
        Err(e) => {
            tracing::error!("Git HTTP backend error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

/// Run git-http-backend as a CGI process.
async fn run_git_cgi(
    repo_path: &std::path::Path,
    method: &Method,
    path: &str,
    query_string: &str,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(StatusCode, Vec<(String, String)>, Vec<u8>), anyhow::Error> {
    // Find git-http-backend
    let git_backend = find_git_http_backend()?;

    // Set up CGI environment variables
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let content_length = body.len().to_string();

    // Build the CGI process
    let mut cmd = Command::new(&git_backend);
    cmd.env("REQUEST_METHOD", method.as_str())
        .env("PATH_INFO", path)
        .env("QUERY_STRING", query_string)
        .env("CONTENT_TYPE", content_type)
        .env("CONTENT_LENGTH", &content_length)
        .env("GIT_PROJECT_ROOT", repo_path)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        // Allow push operations
        .env("REMOTE_USER", "git")
        .env("REMOTE_ADDR", "127.0.0.1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Add HTTP headers as environment variables
    for (key, value) in headers.iter() {
        let key_str = key.as_str().to_uppercase().replace('-', "_");
        if key_str != "CONTENT_TYPE" && key_str != "CONTENT_LENGTH" {
            if let Ok(value_str) = value.to_str() {
                cmd.env(format!("HTTP_{}", key_str), value_str);
            }
        }
    }

    let mut child = cmd.spawn()?;

    // Write request body to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(body).await?;
        drop(stdin);
    }

    // Read stdout and stderr
    let output = child.wait_with_output().await?;

    if !output.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::debug!("git-http-backend stderr: {}", stderr);
    }

    // Parse CGI response (headers + body separated by \r\n\r\n or \n\n)
    let stdout = output.stdout;
    let (status, response_headers, body) = parse_cgi_response(&stdout)?;

    Ok((status, response_headers, body))
}

/// Parse CGI response format: headers separated from body by blank line.
fn parse_cgi_response(
    data: &[u8],
) -> Result<(StatusCode, Vec<(String, String)>, Vec<u8>), anyhow::Error> {
    // Find the header/body separator
    let separator_pos = find_header_separator(data);

    let (header_bytes, body) = match separator_pos {
        Some((pos, len)) => (&data[..pos], data[pos + len..].to_vec()),
        None => (data, Vec::new()),
    };

    // Parse headers
    let header_str = String::from_utf8_lossy(header_bytes);
    let mut status = StatusCode::OK;
    let mut headers = Vec::new();

    for line in header_str.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_lowercase();
            let value = value.trim().to_string();

            if key == "status" {
                // Parse "200 OK" or just "200"
                let code = value
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(200);
                status = StatusCode::from_u16(code).unwrap_or(StatusCode::OK);
            } else {
                headers.push((key, value));
            }
        }
    }

    Ok((status, headers, body))
}

/// Find the position and length of the header/body separator.
fn find_header_separator(data: &[u8]) -> Option<(usize, usize)> {
    // Try \r\n\r\n first
    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
        return Some((pos, 4));
    }

    // Try \n\n
    if let Some(pos) = data.windows(2).position(|w| w == b"\n\n") {
        return Some((pos, 2));
    }

    None
}

/// Find the git-http-backend executable.
fn find_git_http_backend() -> Result<PathBuf, anyhow::Error> {
    // Common locations for git-http-backend
    let candidates = [
        "/usr/lib/git-core/git-http-backend",
        "/usr/libexec/git-core/git-http-backend",
        "/usr/local/lib/git-core/git-http-backend",
        "/opt/homebrew/lib/git-core/git-http-backend",
        "/usr/local/libexec/git-core/git-http-backend",
    ];

    for path in &candidates {
        let path = PathBuf::from(path);
        if path.exists() {
            return Ok(path);
        }
    }

    // Try to find it using git --exec-path
    let output = std::process::Command::new("git")
        .arg("--exec-path")
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let exec_path = String::from_utf8_lossy(&output.stdout);
            let backend = PathBuf::from(exec_path.trim()).join("git-http-backend");
            if backend.exists() {
                return Ok(backend);
            }
        }
    }

    Err(anyhow::anyhow!(
        "git-http-backend not found. Please ensure git is installed."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cgi_response() {
        let response = b"Status: 200 OK\r\nContent-Type: text/plain\r\n\r\nHello, World!";
        let (status, headers, body) = parse_cgi_response(response).unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].0, "content-type");
        assert_eq!(headers[0].1, "text/plain");
        assert_eq!(body, b"Hello, World!");
    }

    #[test]
    fn test_parse_cgi_response_newline() {
        let response = b"Content-Type: application/x-git\n\nbinary data";
        let (status, headers, body) = parse_cgi_response(response).unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.len(), 1);
        assert_eq!(body, b"binary data");
    }

    #[test]
    fn test_find_header_separator() {
        assert_eq!(find_header_separator(b"foo\r\n\r\nbar"), Some((3, 4)));
        assert_eq!(find_header_separator(b"foo\n\nbar"), Some((3, 2)));
        assert_eq!(find_header_separator(b"no separator"), None);
    }
}
