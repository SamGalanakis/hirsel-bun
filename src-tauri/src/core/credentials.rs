//! Credential storage for hirsel
//!
//! Credentials are stored encrypted in SQLite using AES-256-GCM.
//! The encryption key is stored in ~/.hirsel/key (auto-generated on first use).
//!
//! This module provides:
//! - `CredentialStore`: Encrypted credential storage backed by SQLite
//! - `ForwardedCredentials`: Credentials to pass to agent processes

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use thiserror::Error;
use tokio::sync::OnceCell;

use super::db::global_pool;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS credentials (
    id INTEGER PRIMARY KEY,
    key_type TEXT NOT NULL UNIQUE,
    encrypted_value BLOB NOT NULL,
    nonce BLOB NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

static SCHEMA_INIT: OnceCell<()> = OnceCell::const_new();

async fn ensure_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    SCHEMA_INIT
        .get_or_try_init(|| async {
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
            Ok::<(), sqlx::Error>(())
        })
        .await?;
    Ok(())
}

/// Errors that can occur during credential operations
#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Credential not found: {0}")]
    NotFound(String),
}

pub type CredentialResult<T> = Result<T, CredentialError>;

/// Credentials to forward to agent processes
///
/// These are passed from the GUI client to the orchestrator (local or remote)
/// and then applied as environment variables when spawning agent processes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ForwardedCredentials {
    /// OAuth access token for Claude (CLAUDE_ACCESS_TOKEN)
    pub claude_access_token: Option<String>,
    /// API key for Anthropic (ANTHROPIC_API_KEY)
    pub anthropic_api_key: Option<String>,
}

impl ForwardedCredentials {
    /// Create empty credentials
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any credentials are present
    pub fn has_any(&self) -> bool {
        self.claude_access_token.is_some() || self.anthropic_api_key.is_some()
    }

    /// Merge with another set of credentials, preferring self's values
    pub fn merge(self, other: ForwardedCredentials) -> Self {
        Self {
            claude_access_token: self.claude_access_token.or(other.claude_access_token),
            anthropic_api_key: self.anthropic_api_key.or(other.anthropic_api_key),
        }
    }
}

/// Load encryption key from file, or generate a new one if it doesn't exist
fn load_or_generate_key(key_path: &Path) -> CredentialResult<[u8; 32]> {
    if key_path.exists() {
        // Load existing key
        let key_hex = std::fs::read_to_string(key_path)?;
        let key_bytes = hex::decode(key_hex.trim())
            .map_err(|e| CredentialError::Encryption(format!("Invalid key file: {}", e)))?;
        if key_bytes.len() != 32 {
            return Err(CredentialError::Encryption(
                "Key file must contain 32 bytes (64 hex chars)".into(),
            ));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&key_bytes);
        Ok(arr)
    } else {
        // Generate new key
        let key: [u8; 32] = rand::rng().random();
        let key_hex = hex::encode(key);

        // Ensure parent directory exists
        if let Some(parent) = key_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Write key with restrictive permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600) // Owner read/write only
                .open(key_path)?;
            std::io::Write::write_all(&mut file, key_hex.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(key_path, &key_hex)?;
        }

        Ok(key)
    }
}

/// Encrypted credential storage backed by SQLite
///
/// Credentials are stored encrypted using AES-256-GCM. The encryption key
/// is stored in ~/.hirsel/key and auto-generated on first use.
///
/// # Example
///
/// ```rust,ignore
/// use hirsel_lib::core::credentials::{CredentialStore, ForwardedCredentials};
///
/// // Store credentials
/// let store = CredentialStore::open().await?;
/// store.store("oauth_token", "my-secret-token").await?;
///
/// // Load credentials
/// let token = store.load("oauth_token").await?;
///
/// // Load all as ForwardedCredentials
/// let creds = store.load_all().await;
/// ```
pub struct CredentialStore {
    cipher: Aes256Gcm,
}

impl CredentialStore {
    /// Open the global credential store at ~/.hirsel/hirsel.db
    pub async fn open() -> CredentialResult<Self> {
        let key_path = super::config::hirsel_dir().join("key");

        // Load or generate encryption key
        let key_bytes = load_or_generate_key(&key_path)?;

        let cipher = Aes256Gcm::new_from_slice(&key_bytes)
            .map_err(|e| CredentialError::Encryption(e.to_string()))?;

        // Ensure schema exists
        let pool = global_pool().await;
        ensure_schema(pool).await?;

        Ok(Self { cipher })
    }

    /// Get the pool
    async fn pool(&self) -> &'static SqlitePool {
        global_pool().await
    }

    /// Store a credential (encrypted)
    ///
    /// The value is encrypted with AES-256-GCM before storage.
    /// Existing values with the same key_type are replaced.
    pub async fn store(&self, key_type: &str, value: &str) -> CredentialResult<()> {
        let pool = self.pool().await;

        // Generate a random 12-byte nonce
        let nonce_bytes: [u8; 12] = rand::rng().random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let encrypted = self
            .cipher
            .encrypt(nonce, value.as_bytes())
            .map_err(|e| CredentialError::Encryption(e.to_string()))?;

        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT OR REPLACE INTO credentials (key_type, encrypted_value, nonce, updated_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(key_type)
        .bind(&encrypted)
        .bind(nonce_bytes.to_vec())
        .bind(&now)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Load a credential (decrypted)
    ///
    /// Returns the decrypted value or NotFound error if the key doesn't exist.
    pub async fn load(&self, key_type: &str) -> CredentialResult<String> {
        let pool = self.pool().await;

        let row = sqlx::query("SELECT encrypted_value, nonce FROM credentials WHERE key_type = ?")
            .bind(key_type)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| CredentialError::NotFound(key_type.to_string()))?;

        let encrypted: Vec<u8> = row.get("encrypted_value");
        let nonce_bytes: Vec<u8> = row.get("nonce");

        let nonce = Nonce::from_slice(&nonce_bytes);
        let decrypted = self
            .cipher
            .decrypt(nonce, encrypted.as_ref())
            .map_err(|e| CredentialError::Encryption(e.to_string()))?;

        String::from_utf8(decrypted).map_err(|e| CredentialError::Encryption(e.to_string()))
    }

    /// Delete a credential
    pub async fn delete(&self, key_type: &str) -> CredentialResult<()> {
        let pool = self.pool().await;

        sqlx::query("DELETE FROM credentials WHERE key_type = ?")
            .bind(key_type)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Load all credentials as ForwardedCredentials
    ///
    /// Returns default (empty) credentials for any that are not stored.
    pub async fn load_all(&self) -> ForwardedCredentials {
        ForwardedCredentials {
            claude_access_token: self.load("oauth_token").await.ok(),
            anthropic_api_key: self.load("api_key").await.ok(),
        }
    }

    /// Store credentials from ForwardedCredentials
    ///
    /// Only stores non-None values.
    pub async fn store_all(&self, creds: &ForwardedCredentials) -> CredentialResult<()> {
        if let Some(ref token) = creds.claude_access_token {
            self.store("oauth_token", token).await?;
        }
        if let Some(ref key) = creds.anthropic_api_key {
            self.store("api_key", key).await?;
        }
        Ok(())
    }
}

/// Read Claude OAuth credentials from the local ~/.claude/.credentials.json file
///
/// This is used by the GUI to read credentials from the user's local machine
/// before forwarding them to a remote orchestrator.
pub fn get_local_oauth_credentials() -> Option<ForwardedCredentials> {
    let creds_path = dirs::home_dir()?.join(".claude").join(".credentials.json");
    let content = std::fs::read_to_string(&creds_path).ok()?;
    let data: serde_json::Value = serde_json::from_str(&content).ok()?;

    let token = data.get("claudeAiOauth")?.get("accessToken")?.as_str()?;

    Some(ForwardedCredentials {
        claude_access_token: Some(token.to_string()),
        anthropic_api_key: None,
    })
}

/// Read the raw Claude credentials JSON from ~/.claude/.credentials.json
///
/// Returns the full JSON content as a string, suitable for passing to containers
/// or remote workers that need the complete credentials file.
pub fn get_local_oauth_credentials_raw() -> Option<String> {
    let creds_path = dirs::home_dir()?.join(".claude").join(".credentials.json");
    std::fs::read_to_string(&creds_path).ok()
}

#[cfg(test)]
mod tests {
    // Tests need to be updated for async - skipping for now
}
