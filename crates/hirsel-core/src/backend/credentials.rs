//! Credential storage for Hirsel.
//!
//! Credentials are stored encrypted in the embedded SurrealDB database using
//! AES-256-GCM. The encryption key is stored in `~/.hirsel/key` and generated
//! on first use.
//!
//! This module provides:
//! - `CredentialStore`: Encrypted credential storage backed by SurrealDB
//! - `ForwardedCredentials`: Credentials to pass to agent processes

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::Engine;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::Path;
use surrealdb::types::SurrealValue;
use thiserror::Error;

use super::db::{global_db, DbClient};

const CREDENTIAL_TABLE: &str = "credential";

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct CredentialRecord {
    key_type: String,
    encrypted_value_b64: String,
    nonce_b64: String,
    updated_at: String,
}

/// Errors that can occur during credential operations.
#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("Database error: {0}")]
    Database(#[from] surrealdb::Error),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Credential not found: {0}")]
    NotFound(String),
}

pub type CredentialResult<T> = Result<T, CredentialError>;

/// Stored Codex OAuth credential bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOAuthCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    pub account_id: Option<String>,
}

/// Credentials to forward into shepherd and thread container sessions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ForwardedCredentials {
    /// API key for OpenAI-compatible auth (OPENAI_API_KEY)
    pub openai_api_key: Option<String>,
    /// API key for OpenRouter auth (OPENROUTER_API_KEY)
    pub openrouter_api_key: Option<String>,
    /// API key for Tavily web search/fetch tools (TAVILY_API_KEY)
    pub tavily_api_key: Option<String>,
    /// GitHub token for git/gh auth (GITHUB_TOKEN / GH_TOKEN)
    pub github_token: Option<String>,
    /// Codex OAuth access token (CODEX_ACCESS_TOKEN)
    pub codex_access_token: Option<String>,
    /// Codex OAuth refresh token (CODEX_REFRESH_TOKEN)
    pub codex_refresh_token: Option<String>,
    /// Codex token expiry epoch seconds (CODEX_EXPIRES_AT)
    pub codex_expires_at: Option<String>,
    /// Optional Codex account ID (CODEX_ACCOUNT_ID)
    pub codex_account_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    Env,
    Store,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTavilyApiKey {
    pub api_key: String,
    pub source: CredentialSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedGithubToken {
    pub token: String,
    pub source: CredentialSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedCodexOAuthCredentials {
    pub credentials: CodexOAuthCredentials,
    pub source: CredentialSource,
}

impl CredentialSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Store => "store",
        }
    }
}

fn read_env_credential(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Try to read a GitHub token from `gh auth token` (GitHub CLI).
///
/// This covers the common case where the user has run `gh auth login` on the
/// host but has not exported GITHUB_TOKEN / GH_TOKEN into the environment.
fn read_gh_cli_token() -> Option<String> {
    std::process::Command::new("gh")
        .args(["auth", "token"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

impl ForwardedCredentials {
    /// Create empty credentials.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any credentials are present.
    pub fn has_any(&self) -> bool {
        self.openai_api_key.is_some()
            || self.openrouter_api_key.is_some()
            || self.tavily_api_key.is_some()
            || self.github_token.is_some()
            || self.codex_access_token.is_some()
            || self.codex_refresh_token.is_some()
            || self.codex_expires_at.is_some()
            || self.codex_account_id.is_some()
    }

    /// Merge with another set of credentials, preferring self's values.
    pub fn merge(self, other: ForwardedCredentials) -> Self {
        Self {
            openai_api_key: self.openai_api_key.or(other.openai_api_key),
            openrouter_api_key: self.openrouter_api_key.or(other.openrouter_api_key),
            tavily_api_key: self.tavily_api_key.or(other.tavily_api_key),
            github_token: self.github_token.or(other.github_token),
            codex_access_token: self.codex_access_token.or(other.codex_access_token),
            codex_refresh_token: self.codex_refresh_token.or(other.codex_refresh_token),
            codex_expires_at: self.codex_expires_at.or(other.codex_expires_at),
            codex_account_id: self.codex_account_id.or(other.codex_account_id),
        }
    }

    /// Load forwarded credentials from the current process environment.
    pub fn from_env() -> Self {
        Self {
            openai_api_key: read_env_credential("OPENAI_API_KEY"),
            openrouter_api_key: read_env_credential("OPENROUTER_API_KEY"),
            tavily_api_key: read_env_credential("TAVILY_API_KEY"),
            github_token: read_env_credential("GITHUB_TOKEN")
                .or_else(|| read_env_credential("GH_TOKEN"))
                .or_else(read_gh_cli_token),
            codex_access_token: read_env_credential("CODEX_ACCESS_TOKEN"),
            codex_refresh_token: read_env_credential("CODEX_REFRESH_TOKEN"),
            codex_expires_at: read_env_credential("CODEX_EXPIRES_AT"),
            codex_account_id: read_env_credential("CODEX_ACCOUNT_ID"),
        }
    }
}

/// Best-effort load of credentials that should be forwarded to coding sessions.
///
/// Ambient environment values win over stored fallbacks.
pub async fn load_forwarded_credentials() -> ForwardedCredentials {
    let env = ForwardedCredentials::from_env();
    match CredentialStore::open().await {
        Ok(store) => env.merge(store.load_all().await),
        Err(_) => env,
    }
}

pub async fn resolve_tavily_api_key() -> Option<ResolvedTavilyApiKey> {
    if let Some(api_key) = read_env_credential("TAVILY_API_KEY") {
        return Some(ResolvedTavilyApiKey {
            api_key,
            source: CredentialSource::Env,
        });
    }

    let store = CredentialStore::open().await.ok()?;
    let api_key = store.load("tavily_api_key").await.ok()?;
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(ResolvedTavilyApiKey {
        api_key: trimmed.to_string(),
        source: CredentialSource::Store,
    })
}

pub async fn resolve_github_token() -> Option<ResolvedGithubToken> {
    if let Some(token) =
        read_env_credential("GITHUB_TOKEN").or_else(|| read_env_credential("GH_TOKEN"))
    {
        return Some(ResolvedGithubToken {
            token,
            source: CredentialSource::Env,
        });
    }

    let store = CredentialStore::open().await.ok()?;
    let token = store.load("github_token").await.ok()?;
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(ResolvedGithubToken {
        token: trimmed.to_string(),
        source: CredentialSource::Store,
    })
}

pub async fn require_tavily_api_key() -> Result<ResolvedTavilyApiKey, String> {
    resolve_tavily_api_key().await.ok_or_else(|| {
        "Tavily is required for Hirsel. Set TAVILY_API_KEY in the environment or save a Tavily key in Settings."
            .to_string()
    })
}

pub async fn resolve_codex_oauth_credentials() -> Option<ResolvedCodexOAuthCredentials> {
    let access_token = read_env_credential("CODEX_ACCESS_TOKEN")
        .or_else(|| read_env_credential("OPENAI_ACCESS_TOKEN"));
    let refresh_token = read_env_credential("CODEX_REFRESH_TOKEN")
        .or_else(|| read_env_credential("OPENAI_REFRESH_TOKEN"));

    if let (Some(access_token), Some(refresh_token)) = (access_token, refresh_token) {
        let expires_at = read_env_credential("CODEX_EXPIRES_AT")
            .or_else(|| read_env_credential("OPENAI_EXPIRES_AT"))
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(u64::MAX);
        let account_id = read_env_credential("CODEX_ACCOUNT_ID")
            .or_else(|| read_env_credential("OPENAI_ACCOUNT_ID"));
        return Some(ResolvedCodexOAuthCredentials {
            credentials: CodexOAuthCredentials {
                access_token,
                refresh_token,
                expires_at,
                account_id,
            },
            source: CredentialSource::Env,
        });
    }

    let store = CredentialStore::open().await.ok()?;
    let credentials = store.load_codex_oauth().await.ok()??;
    Some(ResolvedCodexOAuthCredentials {
        credentials,
        source: CredentialSource::Store,
    })
}

/// Load encryption key from file, or generate a new one if it doesn't exist.
fn load_or_generate_key(key_path: &Path) -> CredentialResult<[u8; 32]> {
    if key_path.exists() {
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
        let key: [u8; 32] = rand::rng().random();
        let key_hex = hex::encode(key);

        if let Some(parent) = key_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
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

/// Encrypted credential storage backed by SurrealDB.
///
/// Credentials are stored encrypted using AES-256-GCM. The encryption key is
/// stored in `~/.hirsel/key` and auto-generated on first use.
pub struct CredentialStore {
    cipher: Aes256Gcm,
}

impl CredentialStore {
    /// Open the global credential store.
    pub async fn open() -> CredentialResult<Self> {
        let key_path = super::config::hirsel_dir().join("key");
        let key_bytes = load_or_generate_key(&key_path)?;
        let cipher = Aes256Gcm::new_from_slice(&key_bytes)
            .map_err(|e| CredentialError::Encryption(e.to_string()))?;

        let _ = global_db().await;

        Ok(Self { cipher })
    }

    async fn db(&self) -> &'static DbClient {
        global_db().await
    }

    /// Store a credential, encrypted at rest.
    pub async fn store(&self, key_type: &str, value: &str) -> CredentialResult<()> {
        let db = self.db().await;
        let nonce_bytes: [u8; 12] = rand::rng().random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let encrypted = self
            .cipher
            .encrypt(nonce, value.as_bytes())
            .map_err(|e| CredentialError::Encryption(e.to_string()))?;

        let record = CredentialRecord {
            key_type: key_type.to_string(),
            encrypted_value_b64: base64::engine::general_purpose::STANDARD.encode(encrypted),
            nonce_b64: base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let _: Option<CredentialRecord> = db
            .upsert((CREDENTIAL_TABLE, key_type))
            .content(record.clone())
            .await?;
        Ok(())
    }

    /// Load a credential and decrypt it.
    pub async fn load(&self, key_type: &str) -> CredentialResult<String> {
        let db = self.db().await;
        let record: Option<CredentialRecord> = db.select((CREDENTIAL_TABLE, key_type)).await?;
        let record = record.ok_or_else(|| CredentialError::NotFound(key_type.to_string()))?;

        let encrypted = base64::engine::general_purpose::STANDARD
            .decode(record.encrypted_value_b64)
            .map_err(|e| CredentialError::Encryption(format!("Invalid encrypted value: {}", e)))?;
        let nonce_bytes = base64::engine::general_purpose::STANDARD
            .decode(record.nonce_b64)
            .map_err(|e| CredentialError::Encryption(format!("Invalid nonce: {}", e)))?;

        let nonce = Nonce::from_slice(&nonce_bytes);
        let decrypted = self
            .cipher
            .decrypt(nonce, encrypted.as_ref())
            .map_err(|e| CredentialError::Encryption(e.to_string()))?;

        String::from_utf8(decrypted).map_err(|e| CredentialError::Encryption(e.to_string()))
    }

    /// Delete a credential.
    pub async fn delete(&self, key_type: &str) -> CredentialResult<()> {
        let db = self.db().await;
        let _: Option<CredentialRecord> = db.delete((CREDENTIAL_TABLE, key_type)).await?;
        Ok(())
    }

    /// Load all credentials as `ForwardedCredentials`.
    pub async fn load_all(&self) -> ForwardedCredentials {
        ForwardedCredentials {
            openai_api_key: self.load("openai_api_key").await.ok(),
            openrouter_api_key: self.load("openrouter_api_key").await.ok(),
            tavily_api_key: self.load("tavily_api_key").await.ok(),
            github_token: self.load("github_token").await.ok(),
            codex_access_token: self.load("codex_access_token").await.ok(),
            codex_refresh_token: self.load("codex_refresh_token").await.ok(),
            codex_expires_at: self.load("codex_expires_at").await.ok(),
            codex_account_id: self.load("codex_account_id").await.ok(),
        }
    }

    /// Store credentials from `ForwardedCredentials`.
    pub async fn store_all(&self, creds: &ForwardedCredentials) -> CredentialResult<()> {
        if let Some(ref key) = creds.openai_api_key {
            self.store("openai_api_key", key).await?;
        }
        if let Some(ref key) = creds.openrouter_api_key {
            self.store("openrouter_api_key", key).await?;
        }
        if let Some(ref key) = creds.tavily_api_key {
            self.store("tavily_api_key", key).await?;
        }
        if let Some(ref token) = creds.github_token {
            self.store("github_token", token).await?;
        }
        if let Some(ref token) = creds.codex_access_token {
            self.store("codex_access_token", token).await?;
        }
        if let Some(ref token) = creds.codex_refresh_token {
            self.store("codex_refresh_token", token).await?;
        }
        if let Some(ref expires_at) = creds.codex_expires_at {
            self.store("codex_expires_at", expires_at).await?;
        }
        if let Some(ref account_id) = creds.codex_account_id {
            self.store("codex_account_id", account_id).await?;
        }
        Ok(())
    }

    /// Store Codex OAuth credentials.
    pub async fn store_codex_oauth(&self, creds: &CodexOAuthCredentials) -> CredentialResult<()> {
        self.store("codex_access_token", &creds.access_token)
            .await?;
        self.store("codex_refresh_token", &creds.refresh_token)
            .await?;
        self.store("codex_expires_at", &creds.expires_at.to_string())
            .await?;
        match &creds.account_id {
            Some(account_id) => self.store("codex_account_id", account_id).await?,
            None => {
                let _ = self.delete("codex_account_id").await;
            }
        }
        Ok(())
    }

    /// Load Codex OAuth credentials.
    ///
    /// Returns `Ok(None)` when required Codex fields are not present.
    pub async fn load_codex_oauth(&self) -> CredentialResult<Option<CodexOAuthCredentials>> {
        let access_token = match self.load("codex_access_token").await {
            Ok(v) => v,
            Err(CredentialError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        let refresh_token = match self.load("codex_refresh_token").await {
            Ok(v) => v,
            Err(CredentialError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        let expires_at = match self.load("codex_expires_at").await {
            Ok(v) => v.parse::<u64>().map_err(|e| {
                CredentialError::Encryption(format!("Invalid codex_expires_at: {}", e))
            })?,
            Err(CredentialError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        let account_id = self.load("codex_account_id").await.ok();

        Ok(Some(CodexOAuthCredentials {
            access_token,
            refresh_token,
            expires_at,
            account_id,
        }))
    }

    /// Delete all Codex OAuth credentials.
    pub async fn delete_codex_oauth(&self) -> CredentialResult<()> {
        let _ = self.delete("codex_access_token").await;
        let _ = self.delete("codex_refresh_token").await;
        let _ = self.delete("codex_expires_at").await;
        let _ = self.delete("codex_account_id").await;
        Ok(())
    }

    /// Store OpenRouter API key.
    pub async fn store_openrouter_api_key(&self, api_key: &str) -> CredentialResult<()> {
        self.store("openrouter_api_key", api_key).await
    }

    /// Load OpenRouter API key.
    pub async fn load_openrouter_api_key(&self) -> CredentialResult<Option<String>> {
        match self.load("openrouter_api_key").await {
            Ok(v) => Ok(Some(v)),
            Err(CredentialError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Delete OpenRouter API key.
    pub async fn delete_openrouter_api_key(&self) -> CredentialResult<()> {
        let _ = self.delete("openrouter_api_key").await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Tests need to be updated for async - skipping for now.
}
