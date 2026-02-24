//! Tailscale API client for generating ephemeral auth keys
//!
//! Uses Tailscale OAuth to generate short-lived auth keys for workers
//! to join the user's tailnet. Also monitors and renews the orchestrator's
//! own Tailscale auth before it expires.

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn};

use super::http_client::ResponseExt;

/// Tailscale API errors
#[derive(Debug, Error)]
pub enum TailscaleError {
    #[error("OAuth token request failed: {0}")]
    OAuthFailed(String),

    #[error("Auth key generation failed: {0}")]
    KeyGenerationFailed(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Failed to get Tailscale status: {0}")]
    StatusFailed(String),

    #[error("Failed to re-authenticate: {0}")]
    ReauthFailed(String),
}

pub type TailscaleResult<T> = Result<T, TailscaleError>;

/// Tailscale OAuth client for generating auth keys
pub struct TailscaleClient {
    http: Client,
    client_id: String,
    client_secret: String,
    tag: Option<String>,
}

/// OAuth token response
#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(rename = "token_type")]
    _token_type: String,
    #[serde(rename = "expires_in")]
    _expires_in: u64,
}

/// Auth key creation request
#[derive(Debug, Serialize)]
struct CreateKeyRequest {
    capabilities: KeyCapabilities,
    #[serde(rename = "expirySeconds")]
    expiry_seconds: u64,
}

#[derive(Debug, Serialize)]
struct KeyCapabilities {
    devices: DeviceCapabilities,
}

#[derive(Debug, Serialize)]
struct DeviceCapabilities {
    create: DeviceCreateCapabilities,
}

#[derive(Debug, Serialize)]
struct DeviceCreateCapabilities {
    reusable: bool,
    ephemeral: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
}

/// Auth key response
#[derive(Debug, Deserialize)]
struct CreateKeyResponse {
    key: String,
}

impl TailscaleClient {
    /// Create a new Tailscale client with OAuth credentials
    pub fn new(client_id: String, client_secret: String, tag: Option<String>) -> Self {
        Self {
            http: Client::new(),
            client_id,
            client_secret,
            tag,
        }
    }

    /// Get an OAuth access token
    async fn get_access_token(&self) -> TailscaleResult<String> {
        debug!("Requesting Tailscale OAuth token");

        let token_response: OAuthTokenResponse = self
            .http
            .post("https://api.tailscale.com/api/v2/oauth/token")
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("grant_type=client_credentials")
            .send()
            .await?
            .json_or_error()
            .await
            .map_err(|e| TailscaleError::OAuthFailed(e.to_string()))?;

        Ok(token_response.access_token)
    }

    /// Generate a short-lived, ephemeral auth key for a worker
    ///
    /// The key will:
    /// - Expire in 5 minutes (enough time to join)
    /// - Be single-use (not reusable)
    /// - Create ephemeral devices (auto-removed when offline)
    pub async fn generate_auth_key(&self, hostname: &str) -> TailscaleResult<String> {
        info!("Generating Tailscale auth key for {}", hostname);

        // Get OAuth token
        let access_token = self.get_access_token().await?;

        // Build tags list
        let tags = self
            .tag
            .as_ref()
            .map(|t| vec![t.clone()])
            .unwrap_or_default();

        // Create auth key request
        let request = CreateKeyRequest {
            capabilities: KeyCapabilities {
                devices: DeviceCapabilities {
                    create: DeviceCreateCapabilities {
                        reusable: false,
                        ephemeral: true,
                        tags,
                    },
                },
            },
            expiry_seconds: 300, // 5 minutes
        };

        let key_response: CreateKeyResponse = self
            .http
            .post("https://api.tailscale.com/api/v2/tailnet/-/keys")
            .bearer_auth(&access_token)
            .json(&request)
            .send()
            .await?
            .json_or_error()
            .await
            .map_err(|e| TailscaleError::KeyGenerationFailed(e.to_string()))?;

        info!(
            "Generated Tailscale auth key for {} (expires in 5 min)",
            hostname
        );

        Ok(key_response.key)
    }

    /// Generate a non-ephemeral auth key for the orchestrator itself
    ///
    /// Unlike worker keys, this creates a persistent device that won't
    /// auto-remove when offline.
    pub async fn generate_orchestrator_auth_key(&self) -> TailscaleResult<String> {
        info!("Generating Tailscale auth key for orchestrator re-auth");

        let access_token = self.get_access_token().await?;

        // Build tags list
        let tags = self
            .tag
            .as_ref()
            .map(|t| vec![t.clone()])
            .unwrap_or_default();

        // Create auth key request - NOT ephemeral, longer expiry
        let request = CreateKeyRequest {
            capabilities: KeyCapabilities {
                devices: DeviceCapabilities {
                    create: DeviceCreateCapabilities {
                        reusable: false,
                        ephemeral: false, // Orchestrator should persist
                        tags,
                    },
                },
            },
            expiry_seconds: 300, // 5 minutes to use the key
        };

        let key_response: CreateKeyResponse = self
            .http
            .post("https://api.tailscale.com/api/v2/tailnet/-/keys")
            .bearer_auth(&access_token)
            .json(&request)
            .send()
            .await?
            .json_or_error()
            .await
            .map_err(|e| TailscaleError::KeyGenerationFailed(e.to_string()))?;

        info!("Generated Tailscale orchestrator auth key (expires in 5 min)");

        Ok(key_response.key)
    }

    /// Check if Tailscale key is expiring soon and re-authenticate if needed
    ///
    /// Returns Ok(true) if re-auth was performed, Ok(false) if not needed.
    pub async fn check_and_renew_if_needed(&self, renew_before: Duration) -> TailscaleResult<bool> {
        let status = get_tailscale_status()?;

        let Some(key_expiry) = status.self_node.key_expiry else {
            debug!("Tailscale key has no expiry (key expiry disabled)");
            return Ok(false);
        };

        let now = Utc::now();
        let time_until_expiry = key_expiry.signed_duration_since(now);

        if time_until_expiry.num_seconds() < 0 {
            warn!("Tailscale key has already expired, re-authenticating");
            self.reauthenticate().await?;
            return Ok(true);
        }

        let threshold =
            chrono::Duration::from_std(renew_before).unwrap_or(chrono::Duration::hours(24));

        if time_until_expiry < threshold {
            info!(
                "Tailscale key expires in {} hours, re-authenticating",
                time_until_expiry.num_hours()
            );
            self.reauthenticate().await?;
            return Ok(true);
        }

        debug!(
            "Tailscale key valid for {} more hours",
            time_until_expiry.num_hours()
        );
        Ok(false)
    }

    /// Re-authenticate with Tailscale using a fresh auth key
    async fn reauthenticate(&self) -> TailscaleResult<()> {
        let auth_key = self.generate_orchestrator_auth_key().await?;

        let output = Command::new("tailscale")
            .args(["up", "--authkey", &auth_key])
            .output()
            .map_err(|e| {
                TailscaleError::ReauthFailed(format!("Failed to run tailscale up: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(TailscaleError::ReauthFailed(format!(
                "tailscale up failed: {}",
                stderr
            )));
        }

        info!("Successfully re-authenticated with Tailscale");
        Ok(())
    }
}

// =============================================================================
// Tailscale Status Parsing
// =============================================================================

/// Parsed output from `tailscale status --json`
#[derive(Debug, Deserialize)]
pub struct TailscaleStatus {
    #[serde(rename = "Self")]
    pub self_node: TailscaleSelfNode,
    #[serde(rename = "BackendState")]
    pub backend_state: String,
}

#[derive(Debug, Deserialize)]
pub struct TailscaleSelfNode {
    #[serde(rename = "HostName")]
    pub hostname: String,
    #[serde(rename = "DNSName")]
    pub dns_name: String,
    #[serde(rename = "TailscaleIPs")]
    pub tailscale_ips: Vec<String>,
    #[serde(rename = "KeyExpiry", default)]
    pub key_expiry: Option<DateTime<Utc>>,
}

/// Get current Tailscale status by running `tailscale status --json`
pub fn get_tailscale_status() -> TailscaleResult<TailscaleStatus> {
    let output = Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .map_err(|e| TailscaleError::StatusFailed(format!("Failed to run tailscale: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(TailscaleError::StatusFailed(format!(
            "tailscale status failed: {}",
            stderr
        )));
    }

    let status: TailscaleStatus = serde_json::from_slice(&output.stdout)
        .map_err(|e| TailscaleError::StatusFailed(format!("Failed to parse status: {}", e)))?;

    Ok(status)
}

/// Check if Tailscale is currently connected
pub fn is_tailscale_connected() -> bool {
    match get_tailscale_status() {
        Ok(status) => status.backend_state == "Running",
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_key_request_serialization() {
        let request = CreateKeyRequest {
            capabilities: KeyCapabilities {
                devices: DeviceCapabilities {
                    create: DeviceCreateCapabilities {
                        reusable: false,
                        ephemeral: true,
                        tags: vec!["tag:hirsel-worker".to_string()],
                    },
                },
            },
            expiry_seconds: 300,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("ephemeral"));
        assert!(json.contains("tag:hirsel-worker"));
        assert!(json.contains("300"));
    }

    #[test]
    fn test_create_key_request_no_tags() {
        let request = CreateKeyRequest {
            capabilities: KeyCapabilities {
                devices: DeviceCapabilities {
                    create: DeviceCreateCapabilities {
                        reusable: false,
                        ephemeral: true,
                        tags: vec![],
                    },
                },
            },
            expiry_seconds: 300,
        };

        let json = serde_json::to_string(&request).unwrap();
        // tags should be omitted when empty
        assert!(!json.contains("tags"));
    }
}
