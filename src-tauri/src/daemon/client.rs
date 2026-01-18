//! Client for communicating with the hirsel daemon
//!
//! Provides a simple HTTP client that connects via Unix socket.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;

/// Client for the hirsel daemon
#[derive(Clone)]
pub struct DaemonClient {
    /// Unix socket path
    socket_path: PathBuf,
    /// HTTP client configured for Unix socket
    client: Client,
}

impl DaemonClient {
    /// Create a new daemon client
    fn new(socket_path: PathBuf) -> Result<Self> {
        // Build a client that can connect to Unix sockets
        // We use hyper-util with unix socket connector
        let client = Client::builder()
            .timeout(Duration::from_secs(300)) // 5 minute timeout for long operations
            .build()?;

        Ok(Self {
            socket_path,
            client,
        })
    }

    /// Connect to an existing daemon
    pub fn connect() -> Result<Self> {
        let socket_path = super::socket_path();

        if !socket_path.exists() {
            return Err(anyhow!(
                "Daemon socket not found at {}",
                socket_path.display()
            ));
        }

        // Test connection
        if !super::is_daemon_running() {
            return Err(anyhow!("Daemon is not responding"));
        }

        Self::new(socket_path)
    }

    /// Connect to daemon, starting it if needed
    pub fn connect_or_start() -> Result<Self> {
        match Self::connect() {
            Ok(client) => Ok(client),
            Err(_) => {
                Self::start_daemon()?;
                // Wait for daemon to be ready
                for i in 0..50 {
                    std::thread::sleep(Duration::from_millis(100));
                    if let Ok(client) = Self::connect() {
                        tracing::debug!("Connected to daemon after {}ms", (i + 1) * 100);
                        return Ok(client);
                    }
                }
                Err(anyhow!("Daemon failed to start within 5 seconds"))
            }
        }
    }

    /// Start the daemon process
    fn start_daemon() -> Result<()> {
        use std::process::{Command, Stdio};

        let exe = std::env::current_exe()?;

        tracing::info!("Starting daemon: {} __daemon", exe.display());

        // Spawn daemon in background
        // Use setsid to create a new session (fully detached)
        #[cfg(unix)]
        {
            Command::new(&exe)
                .arg("__daemon")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
        }

        #[cfg(not(unix))]
        {
            Command::new(&exe)
                .arg("__daemon")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
        }

        Ok(())
    }

    /// Make a request to the daemon via Unix socket
    async fn request<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<impl Serialize>,
    ) -> Result<T> {
        // For Unix socket, we need to use a custom transport
        // Since reqwest doesn't natively support Unix sockets,
        // we'll use hyper directly with tokio's UnixStream

        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::UnixStream;

        let mut stream = UnixStream::connect(&self.socket_path).await?;

        // Build HTTP request
        let body_bytes = match body {
            Some(b) => serde_json::to_vec(&b)?,
            None => Vec::new(),
        };

        let request = if body_bytes.is_empty() {
            format!(
                "{} {} HTTP/1.1\r\n\
                 Host: localhost\r\n\
                 Accept: application/json\r\n\
                 Connection: close\r\n\
                 \r\n",
                method.as_str(),
                path
            )
        } else {
            format!(
                "{} {} HTTP/1.1\r\n\
                 Host: localhost\r\n\
                 Accept: application/json\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\
                 \r\n",
                method.as_str(),
                path,
                body_bytes.len()
            )
        };

        // Send request
        stream.write_all(request.as_bytes()).await?;
        if !body_bytes.is_empty() {
            stream.write_all(&body_bytes).await?;
        }

        // Read response
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;

        // Parse HTTP response
        let response_str = String::from_utf8_lossy(&response);

        // Find the body (after \r\n\r\n)
        let body_start = response_str.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);

        let body = &response[body_start..];

        // Check for chunked encoding
        let body = if response_str.contains("Transfer-Encoding: chunked") {
            // Parse chunked encoding
            parse_chunked_body(body)?
        } else {
            body.to_vec()
        };

        // Parse status code
        let status_line = response_str.lines().next().unwrap_or("");
        let status_code = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(500);

        if status_code >= 400 {
            let error_msg = String::from_utf8_lossy(&body);
            return Err(anyhow!("HTTP {}: {}", status_code, error_msg));
        }

        // Handle empty body
        if body.is_empty() {
            // Return default for unit type
            let empty: T = serde_json::from_str("{}")
                .or_else(|_| serde_json::from_str("null"))
                .map_err(|e| anyhow!("Failed to parse empty response: {}", e))?;
            return Ok(empty);
        }

        serde_json::from_slice(&body).map_err(|e| {
            anyhow!(
                "Failed to parse response: {} (body: {})",
                e,
                String::from_utf8_lossy(&body)
            )
        })
    }

    // Convenience methods

    /// GET request
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.request::<T>(reqwest::Method::GET, path, None::<()>)
            .await
    }

    /// POST request with JSON body
    pub async fn post<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: B) -> Result<T> {
        self.request::<T>(reqwest::Method::POST, path, Some(body))
            .await
    }

    /// POST request without body
    pub async fn post_empty<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.request::<T>(reqwest::Method::POST, path, None::<()>)
            .await
    }

    /// DELETE request
    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.request::<T>(reqwest::Method::DELETE, path, None::<()>)
            .await
    }

    /// Check if daemon is healthy
    pub async fn health(&self) -> Result<()> {
        let _: serde_json::Value = self.get("/health").await?;
        Ok(())
    }

    /// Stop the daemon
    pub async fn stop(&self) -> Result<()> {
        let _: serde_json::Value = self.post_empty("/daemon/stop").await?;
        Ok(())
    }
}

/// Parse HTTP chunked transfer encoding
fn parse_chunked_body(data: &[u8]) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut pos = 0;

    loop {
        // Find end of chunk size line
        let line_end = data[pos..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .map(|i| pos + i)
            .ok_or_else(|| anyhow!("Invalid chunked encoding"))?;

        // Parse chunk size (hex)
        let size_str = std::str::from_utf8(&data[pos..line_end])?;
        let chunk_size = usize::from_str_radix(size_str.trim(), 16)
            .map_err(|e| anyhow!("Invalid chunk size '{}': {}", size_str, e))?;

        if chunk_size == 0 {
            break;
        }

        // Read chunk data
        let chunk_start = line_end + 2;
        let chunk_end = chunk_start + chunk_size;

        if chunk_end > data.len() {
            return Err(anyhow!("Chunk extends beyond data"));
        }

        result.extend_from_slice(&data[chunk_start..chunk_end]);

        // Move past chunk and trailing \r\n
        pos = chunk_end + 2;

        if pos >= data.len() {
            break;
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunked_parsing() {
        // Example chunked response: "Hello" (5 bytes) + " World" (6 bytes)
        let data = b"5\r\nHello\r\n6\r\n World\r\n0\r\n\r\n";
        let result = parse_chunked_body(data).unwrap();
        assert_eq!(result, b"Hello World");
    }
}
