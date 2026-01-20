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
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS credentials (
    id INTEGER PRIMARY KEY,
    key_type TEXT NOT NULL UNIQUE,
    encrypted_value BLOB NOT NULL,
    nonce BLOB NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

/// Errors that can occur during credential operations
#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

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
/// let store = CredentialStore::open()?;
/// store.store("oauth_token", "my-secret-token")?;
///
/// // Load credentials
/// let token = store.load("oauth_token")?;
///
/// // Load all as ForwardedCredentials
/// let creds = store.load_all();
/// ```
pub struct CredentialStore {
    db: Connection,
    cipher: Aes256Gcm,
}

impl CredentialStore {
    /// Open the global credential store at ~/.hirsel/hirsel.db
    pub fn open() -> CredentialResult<Self> {
        let key_path = super::config::hirsel_dir().join("key");
        Self::open_with_key(&super::config::global_db_path(), &key_path)
    }

    /// Open a credential store at a specific path with a specific key file
    pub fn open_with_key(db_path: &Path, key_path: &Path) -> CredentialResult<Self> {
        // Load or generate encryption key
        let key_bytes = load_or_generate_key(key_path)?;

        let cipher = Aes256Gcm::new_from_slice(&key_bytes)
            .map_err(|e| CredentialError::Encryption(e.to_string()))?;

        // Open DB
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let db = Connection::open(db_path)?;
        // Enable WAL mode for better concurrent read/write performance
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.execute_batch(SCHEMA)?;

        Ok(Self { db, cipher })
    }

    /// Open a credential store at a specific path (uses default key location)
    pub fn open_at(db_path: &Path) -> CredentialResult<Self> {
        let key_path = super::config::hirsel_dir().join("key");
        Self::open_with_key(db_path, &key_path)
    }

    /// Store a credential (encrypted)
    ///
    /// The value is encrypted with AES-256-GCM before storage.
    /// Existing values with the same key_type are replaced.
    pub fn store(&self, key_type: &str, value: &str) -> CredentialResult<()> {
        // Generate a random 12-byte nonce
        let nonce_bytes: [u8; 12] = rand::rng().random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let encrypted = self
            .cipher
            .encrypt(nonce, value.as_bytes())
            .map_err(|e| CredentialError::Encryption(e.to_string()))?;

        let now = chrono::Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT OR REPLACE INTO credentials (key_type, encrypted_value, nonce, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![key_type, encrypted, nonce_bytes.to_vec(), now],
        )?;
        Ok(())
    }

    /// Load a credential (decrypted)
    ///
    /// Returns the decrypted value or NotFound error if the key doesn't exist.
    pub fn load(&self, key_type: &str) -> CredentialResult<String> {
        let mut stmt = self
            .db
            .prepare("SELECT encrypted_value, nonce FROM credentials WHERE key_type = ?1")?;

        let (encrypted, nonce_bytes): (Vec<u8>, Vec<u8>) = stmt
            .query_row(params![key_type], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|_| CredentialError::NotFound(key_type.to_string()))?;

        let nonce = Nonce::from_slice(&nonce_bytes);
        let decrypted = self
            .cipher
            .decrypt(nonce, encrypted.as_ref())
            .map_err(|e| CredentialError::Encryption(e.to_string()))?;

        String::from_utf8(decrypted).map_err(|e| CredentialError::Encryption(e.to_string()))
    }

    /// Delete a credential
    pub fn delete(&self, key_type: &str) -> CredentialResult<()> {
        self.db.execute(
            "DELETE FROM credentials WHERE key_type = ?1",
            params![key_type],
        )?;
        Ok(())
    }

    /// Load all credentials as ForwardedCredentials
    ///
    /// Returns default (empty) credentials for any that are not stored.
    pub fn load_all(&self) -> ForwardedCredentials {
        ForwardedCredentials {
            claude_access_token: self.load("oauth_token").ok(),
            anthropic_api_key: self.load("api_key").ok(),
        }
    }

    /// Store credentials from ForwardedCredentials
    ///
    /// Only stores non-None values.
    pub fn store_all(&self, creds: &ForwardedCredentials) -> CredentialResult<()> {
        if let Some(ref token) = creds.claude_access_token {
            self.store("oauth_token", token)?;
        }
        if let Some(ref key) = creds.anthropic_api_key {
            self.store("api_key", key)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_env() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");
        let key_path = tmp.path().join("key");
        (tmp, db_path, key_path)
    }

    #[test]
    fn test_store_and_load() {
        let (_tmp, db_path, key_path) = setup_test_env();
        let store = CredentialStore::open_with_key(&db_path, &key_path).unwrap();

        store.store("test_key", "secret_value").unwrap();
        let loaded = store.load("test_key").unwrap();
        assert_eq!(loaded, "secret_value");
    }

    #[test]
    fn test_load_not_found() {
        let (_tmp, db_path, key_path) = setup_test_env();
        let store = CredentialStore::open_with_key(&db_path, &key_path).unwrap();

        let result = store.load("nonexistent");
        assert!(matches!(result, Err(CredentialError::NotFound(_))));
    }

    #[test]
    fn test_store_replace() {
        let (_tmp, db_path, key_path) = setup_test_env();
        let store = CredentialStore::open_with_key(&db_path, &key_path).unwrap();

        store.store("key", "value1").unwrap();
        store.store("key", "value2").unwrap();
        let loaded = store.load("key").unwrap();
        assert_eq!(loaded, "value2");
    }

    #[test]
    fn test_delete() {
        let (_tmp, db_path, key_path) = setup_test_env();
        let store = CredentialStore::open_with_key(&db_path, &key_path).unwrap();

        store.store("to_delete", "value").unwrap();
        store.delete("to_delete").unwrap();
        let result = store.load("to_delete");
        assert!(matches!(result, Err(CredentialError::NotFound(_))));
    }

    #[test]
    fn test_forwarded_credentials_merge() {
        let creds1 = ForwardedCredentials {
            claude_access_token: Some("token1".to_string()),
            anthropic_api_key: None,
        };
        let creds2 = ForwardedCredentials {
            claude_access_token: Some("token2".to_string()),
            anthropic_api_key: Some("key2".to_string()),
        };

        let merged = creds1.merge(creds2);
        assert_eq!(merged.claude_access_token, Some("token1".to_string()));
        assert_eq!(merged.anthropic_api_key, Some("key2".to_string()));
    }

    #[test]
    fn test_forwarded_credentials_has_any() {
        let empty = ForwardedCredentials::default();
        assert!(!empty.has_any());

        let with_token = ForwardedCredentials {
            claude_access_token: Some("token".to_string()),
            anthropic_api_key: None,
        };
        assert!(with_token.has_any());
    }

    #[test]
    fn test_store_all_and_load_all() {
        let (_tmp, db_path, key_path) = setup_test_env();
        let store = CredentialStore::open_with_key(&db_path, &key_path).unwrap();

        let creds = ForwardedCredentials {
            claude_access_token: Some("oauth_token_value".to_string()),
            anthropic_api_key: Some("api_key_value".to_string()),
        };
        store.store_all(&creds).unwrap();

        let loaded = store.load_all();
        assert_eq!(
            loaded.claude_access_token,
            Some("oauth_token_value".to_string())
        );
        assert_eq!(loaded.anthropic_api_key, Some("api_key_value".to_string()));
    }

    #[test]
    fn test_key_generation() {
        let (_tmp, _db_path, key_path) = setup_test_env();

        // Key file shouldn't exist yet
        assert!(!key_path.exists());

        // Generate key
        let key1 = load_or_generate_key(&key_path).unwrap();
        assert!(key_path.exists());

        // Loading again should return the same key
        let key2 = load_or_generate_key(&key_path).unwrap();
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_persistence_across_opens() {
        let (_tmp, db_path, key_path) = setup_test_env();

        // Store a value
        {
            let store = CredentialStore::open_with_key(&db_path, &key_path).unwrap();
            store.store("persistent", "value123").unwrap();
        }

        // Reopen and verify
        {
            let store = CredentialStore::open_with_key(&db_path, &key_path).unwrap();
            let loaded = store.load("persistent").unwrap();
            assert_eq!(loaded, "value123");
        }
    }
}
