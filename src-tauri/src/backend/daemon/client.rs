//! Client for communicating with the hirsel daemon
//!
//! Provides a simple HTTP client that connects via TCP.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::time::Duration;

use crate::backend::http_client::ResponseExt;

use super::{get_daemon_port, DAEMON_PORT_ENV};

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
        let port = get_daemon_port();
        if !super::is_daemon_running_on_port(port) {
            return Err(anyhow!("Daemon is not running on port {}", port));
        }

        Self::new(port)
    }

    /// Connect to daemon, starting it if needed
    ///
    /// Also checks for binary mismatch - if the running daemon was started from
    /// a different binary (e.g., debug vs release), it will be restarted.
    pub fn connect_or_start() -> Result<Self> {
        let port = get_daemon_port();

        // Check if daemon is running but from a different binary
        if super::is_daemon_running_on_port(port) && !super::is_daemon_binary_current() {
            tracing::info!(
                "Daemon binary mismatch detected, restarting daemon with current binary"
            );
            super::kill_daemon();
            // Wait for old daemon to exit
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(100));
                if !super::is_daemon_running_on_port(port) {
                    break;
                }
            }
        }

        match Self::connect() {
            Ok(client) => Ok(client),
            Err(_) => {
                Self::start_daemon(port)?;
                // Wait for daemon to be ready
                for i in 0..50 {
                    std::thread::sleep(Duration::from_millis(100));
                    if super::is_daemon_running_on_port(port) {
                        tracing::debug!("Connected to daemon after {}ms", (i + 1) * 100);
                        return Self::new(port);
                    }
                }
                Err(anyhow!("Daemon failed to start within 5 seconds"))
            }
        }
    }

    /// Start the daemon process
    fn start_daemon(port: u16) -> Result<()> {
        use std::fs::OpenOptions;
        use std::process::{Command, Stdio};

        use crate::backend::config::paths::hirsel_dir;

        let exe = std::env::current_exe()?;

        tracing::info!(
            "Starting daemon: {} __daemon (port {})",
            exe.display(),
            port
        );

        let daemon_log = hirsel_dir().join("daemon.log");
        let stderr_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&daemon_log)
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null());
        let stdout_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&daemon_log)
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null());

        // Spawn daemon in background, passing port via env var
        let mut cmd = Command::new(&exe);
        cmd.arg("__daemon")
            .stdin(Stdio::null())
            .stdout(stdout_file)
            .stderr(stderr_file);

        // Give the daemon its own trace file so it doesn't contend with the GUI's trace.json
        cmd.env("HIRSEL_TRACE_FILENAME", "daemon-trace.json");

        // Pass current environment including HIRSEL_ROOT and HIRSEL_DAEMON_PORT
        if let Ok(root) = std::env::var("HIRSEL_ROOT") {
            cmd.env("HIRSEL_ROOT", root);
        }
        cmd.env(DAEMON_PORT_ENV, port.to_string());

        cmd.spawn()?;

        Ok(())
    }

    /// Make a GET request to the daemon
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        Ok(self.client.get(&url).send().await?.json_or_error().await?)
    }

    /// POST request with JSON body
    pub async fn post<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: B) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        Ok(self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .json_or_error()
            .await?)
    }

    /// POST request without body
    pub async fn post_empty<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        Ok(self.client.post(&url).send().await?.json_or_error().await?)
    }

    /// PATCH request with JSON body
    pub async fn patch<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: B) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        Ok(self
            .client
            .patch(&url)
            .json(&body)
            .send()
            .await?
            .json_or_error()
            .await?)
    }

    /// DELETE request
    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        Ok(self
            .client
            .delete(&url)
            .send()
            .await?
            .json_or_error()
            .await?)
    }

    /// POST request with raw bytes (for file upload)
    pub async fn post_bytes(&self, path: &str, body: Vec<u8>) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);
        self.client
            .post(&url)
            .header("Content-Type", "application/gzip")
            .body(body)
            .send()
            .await?
            .success_or_error()
            .await?;
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
