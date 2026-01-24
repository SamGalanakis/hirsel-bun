//! Client for communicating with the hirsel daemon
//!
//! Provides a simple HTTP client that connects via TCP.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::time::Duration;

use super::DEFAULT_TCP_PORT;

/// Client for the hirsel daemon
#[derive(Clone)]
pub struct DaemonClient {
    /// HTTP client
    client: Client,
    /// Base URL for the daemon
    base_url: String,
}

impl DaemonClient {
    /// Create a new daemon client
    fn new(port: u16) -> Result<Self> {
        let client = Client::builder().timeout(Duration::from_secs(30)).build()?;

        Ok(Self {
            client,
            base_url: format!("http://127.0.0.1:{}", port),
        })
    }

    /// Connect to an existing daemon
    pub fn connect() -> Result<Self> {
        if !super::is_daemon_running() {
            return Err(anyhow!(
                "Daemon is not running on port {}",
                DEFAULT_TCP_PORT
            ));
        }

        Self::new(DEFAULT_TCP_PORT)
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
                    if super::is_daemon_running() {
                        tracing::debug!("Connected to daemon after {}ms", (i + 1) * 100);
                        return Self::new(DEFAULT_TCP_PORT);
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
        Command::new(&exe)
            .arg("__daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        Ok(())
    }

    /// Make a GET request to the daemon
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, body));
        }

        response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse response: {}", e))
    }

    /// POST request with JSON body
    pub async fn post<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: B) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let response = self.client.post(&url).json(&body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, body));
        }

        response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse response: {}", e))
    }

    /// POST request without body
    pub async fn post_empty<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let response = self.client.post(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, body));
        }

        response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse response: {}", e))
    }

    /// DELETE request
    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let response = self.client.delete(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, body));
        }

        response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse response: {}", e))
    }

    /// POST request with raw bytes (for file upload)
    pub async fn post_bytes(&self, path: &str, body: Vec<u8>) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/gzip")
            .body(body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, body));
        }

        Ok(())
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
