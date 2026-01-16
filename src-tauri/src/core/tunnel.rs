//! SSH tunnel management for remote workers.
//!
//! Creates and monitors reverse SSH tunnels so remote workers can connect
//! back to the coordinator's API.

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use thiserror::Error;

// =============================================================================
// Errors
// =============================================================================

#[derive(Debug, Error)]
pub enum TunnelError {
    #[error("SSH tunnel to {host} failed to start: {message}")]
    StartFailed { host: String, message: String },

    #[error("ssh/autossh not found in PATH")]
    SshNotFound,

    #[error("Tunnel to {host} has died")]
    TunnelDied { host: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type TunnelResult<T> = Result<T, TunnelError>;

// =============================================================================
// SSH Tunnel
// =============================================================================

/// Represents an active SSH tunnel to a remote host.
pub struct SSHTunnel {
    pub host: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub ssh_key: Option<String>,
    pub ssh_port: u16,
    process: Child,
}

impl SSHTunnel {
    /// Check if the tunnel process is still running.
    pub fn is_alive(&mut self) -> bool {
        match self.process.try_wait() {
            Ok(None) => true,     // Still running
            Ok(Some(_)) => false, // Exited
            Err(_) => false,      // Error checking - assume dead
        }
    }

    /// Terminate the tunnel process.
    pub fn close(&mut self) {
        tracing::debug!("Closing tunnel to {}", self.host);
        if self.is_alive() {
            // Try graceful termination first
            #[cfg(unix)]
            {
                unsafe {
                    libc::kill(self.process.id() as i32, libc::SIGTERM);
                }
            }

            #[cfg(not(unix))]
            {
                let _ = self.process.kill();
            }

            // Wait for process to exit
            match self.process.wait_timeout(Duration::from_secs(5)) {
                Ok(Some(_)) => {}
                _ => {
                    tracing::warn!("Tunnel to {} didn't terminate, killing", self.host);
                    let _ = self.process.kill();
                }
            }
        }
    }

    /// Get any error output from the tunnel process.
    pub fn get_stderr(&mut self) -> String {
        if let Some(stderr) = self.process.stderr.as_mut() {
            use std::io::Read;
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf);
            buf
        } else {
            String::new()
        }
    }
}

impl Drop for SSHTunnel {
    fn drop(&mut self) {
        self.close();
    }
}

// Extension trait to add wait_timeout to Child
trait ChildExt {
    fn wait_timeout(
        &mut self,
        timeout: Duration,
    ) -> std::io::Result<Option<std::process::ExitStatus>>;
}

impl ChildExt for Child {
    fn wait_timeout(
        &mut self,
        timeout: Duration,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        let start = std::time::Instant::now();
        loop {
            match self.try_wait()? {
                Some(status) => return Ok(Some(status)),
                None => {
                    if start.elapsed() >= timeout {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

// =============================================================================
// Tunnel Manager
// =============================================================================

/// Manages SSH tunnels for remote workers.
///
/// Creates reverse tunnels so remote workers can connect to the coordinator API.
/// Uses autossh for automatic reconnection if available.
pub struct TunnelManager {
    base_port: u16,
    next_port: u16,
    use_autossh: bool,
    tunnels: HashMap<String, SSHTunnel>,
}

impl TunnelManager {
    /// Create a new tunnel manager.
    pub fn new(base_port: u16) -> Self {
        let use_autossh = which::which("autossh").is_ok();
        if use_autossh {
            tracing::info!("autossh available - tunnels will auto-reconnect");
        } else {
            tracing::info!("autossh not found - using standard ssh");
        }

        Self {
            base_port,
            next_port: base_port,
            use_autossh,
            tunnels: HashMap::new(),
        }
    }

    /// Create a reverse SSH tunnel to a remote host.
    ///
    /// # Arguments
    /// * `host` - SSH host (e.g., "user@server.example.com")
    /// * `ssh_key` - Path to SSH private key (optional)
    /// * `ssh_port` - SSH port on remote (default 22)
    /// * `coordinator_port` - Port the coordinator API is listening on locally
    pub fn create_tunnel(
        &mut self,
        host: &str,
        ssh_key: Option<&str>,
        ssh_port: u16,
        coordinator_port: Option<u16>,
    ) -> TunnelResult<&SSHTunnel> {
        // If we already have a working tunnel to this host, return it
        if let Some(tunnel) = self.tunnels.get_mut(host) {
            if tunnel.is_alive() {
                tracing::debug!("Reusing existing tunnel to {}", host);
                return Ok(self.tunnels.get(host).unwrap());
            }
        }

        // Allocate ports
        let local_port = coordinator_port.unwrap_or(self.base_port);
        let remote_port = self.next_port;
        self.next_port += 1;

        // Build SSH command
        let cmd = self.build_ssh_command(host, local_port, remote_port, ssh_key, ssh_port);

        tracing::info!(
            "Creating tunnel to {}: remote:{} -> local:{}",
            host,
            remote_port,
            local_port
        );
        tracing::debug!("Tunnel command: {}", cmd.join(" "));

        // Start the SSH process
        let process = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;

        // Wait briefly and check if process started successfully
        std::thread::sleep(Duration::from_millis(500));

        let mut tunnel = SSHTunnel {
            host: host.to_string(),
            local_port,
            remote_port,
            ssh_key: ssh_key.map(|s| s.to_string()),
            ssh_port,
            process,
        };

        if !tunnel.is_alive() {
            let stderr = tunnel.get_stderr();
            return Err(TunnelError::StartFailed {
                host: host.to_string(),
                message: stderr,
            });
        }

        tracing::info!(
            "Tunnel to {} established (remote port {})",
            host,
            remote_port
        );

        self.tunnels.insert(host.to_string(), tunnel);
        Ok(self.tunnels.get(host).unwrap())
    }

    /// Build the SSH command for creating a reverse tunnel.
    fn build_ssh_command(
        &self,
        host: &str,
        local_port: u16,
        remote_port: u16,
        ssh_key: Option<&str>,
        ssh_port: u16,
    ) -> Vec<String> {
        let mut cmd = if self.use_autossh {
            vec![
                "autossh".to_string(),
                "-M".to_string(),
                "0".to_string(), // Disable autossh's built-in monitoring port
                "-o".to_string(),
                "ServerAliveInterval=30".to_string(),
                "-o".to_string(),
                "ServerAliveCountMax=3".to_string(),
            ]
        } else {
            vec![
                "ssh".to_string(),
                "-o".to_string(),
                "ServerAliveInterval=30".to_string(),
                "-o".to_string(),
                "ServerAliveCountMax=3".to_string(),
            ]
        };

        cmd.extend([
            "-N".to_string(), // No remote command
            "-R".to_string(),
            format!("{}:127.0.0.1:{}", remote_port, local_port), // Reverse tunnel
            "-p".to_string(),
            ssh_port.to_string(),
            "-o".to_string(),
            "ExitOnForwardFailure=yes".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(), // Fail on password prompt
        ]);

        if let Some(key) = ssh_key {
            let key_path = shellexpand::tilde(key);
            cmd.extend(["-i".to_string(), key_path.to_string()]);
        }

        cmd.push(host.to_string());
        cmd
    }

    /// Get an existing tunnel to a host, if alive.
    pub fn get_tunnel(&mut self, host: &str) -> Option<&SSHTunnel> {
        if let Some(tunnel) = self.tunnels.get_mut(host) {
            if tunnel.is_alive() {
                return Some(self.tunnels.get(host).unwrap());
            }
        }
        None
    }

    /// Check tunnel health, return list of dead tunnel hosts.
    pub fn check_tunnels(&mut self) -> Vec<String> {
        let mut dead = Vec::new();

        for (host, tunnel) in self.tunnels.iter_mut() {
            if !tunnel.is_alive() {
                tracing::warn!("Tunnel to {} has died", host);
                let stderr = tunnel.get_stderr();
                if !stderr.is_empty() {
                    tracing::debug!("Tunnel {} stderr: {}", host, stderr);
                }
                dead.push(host.clone());
            }
        }

        // Remove dead tunnels
        for host in &dead {
            self.tunnels.remove(host);
        }

        dead
    }

    /// Attempt to reconnect a dead tunnel.
    pub fn reconnect_tunnel(
        &mut self,
        host: &str,
        ssh_key: Option<&str>,
        ssh_port: u16,
        coordinator_port: Option<u16>,
    ) -> TunnelResult<&SSHTunnel> {
        // Remove existing dead tunnel if present
        if let Some(mut tunnel) = self.tunnels.remove(host) {
            if tunnel.is_alive() {
                // Still alive, put it back
                self.tunnels.insert(host.to_string(), tunnel);
                return Ok(self.tunnels.get(host).unwrap());
            }
            tunnel.close();
        }

        self.create_tunnel(host, ssh_key, ssh_port, coordinator_port)
    }

    /// Close all tunnels.
    pub fn close_all(&mut self) {
        tracing::info!("Closing {} tunnel(s)", self.tunnels.len());
        // Tunnels will be closed when dropped
        self.tunnels.clear();
    }

    /// Get the number of active tunnels.
    pub fn len(&self) -> usize {
        self.tunnels.len()
    }

    /// Check if there are no tunnels.
    pub fn is_empty(&self) -> bool {
        self.tunnels.is_empty()
    }

    /// Get all tunnel hosts and their remote ports.
    pub fn tunnel_ports(&self) -> HashMap<String, u16> {
        self.tunnels
            .iter()
            .map(|(host, tunnel)| (host.clone(), tunnel.remote_port))
            .collect()
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        self.close_all();
    }
}
