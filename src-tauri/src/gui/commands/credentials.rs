//! Credential management commands
//!
//! Commands for storing and managing encrypted credentials.

use super::ResultExt;
use crate::core::credentials::CredentialStore;

/// Store a credential in the encrypted credential store
///
/// This is used for storing API keys that should not be stored
/// in plain text in the config file.
#[tauri::command]
pub async fn store_credential(key_type: String, value: String) -> Result<(), String> {
    let store = CredentialStore::open().await.str_err()?;
    store.store(&key_type, &value).await.str_err()
}

/// Delete a credential from the credential store
#[tauri::command]
pub async fn delete_credential(key_type: String) -> Result<(), String> {
    let store = CredentialStore::open().await.str_err()?;
    store.delete(&key_type).await.str_err()
}

/// Check if a credential exists in the credential store
#[tauri::command]
pub async fn has_credential(key_type: String) -> Result<bool, String> {
    let store = CredentialStore::open().await.str_err()?;
    Ok(store.load(&key_type).await.is_ok())
}

/// Get the actual value of a credential
///
/// Used internally for auth verification. Only accessible from within the app.
#[tauri::command]
pub async fn get_credential(key_type: String) -> Result<Option<String>, String> {
    let store = CredentialStore::open().await.str_err()?;
    match store.load(&key_type).await {
        Ok(value) => Ok(Some(value)),
        Err(_) => Ok(None),
    }
}

/// Get the masked value of a credential (for display purposes)
///
/// Returns the first 4 and last 4 characters of the credential,
/// or "****" if the credential is short.
#[tauri::command]
pub async fn get_credential_masked(key_type: String) -> Result<Option<String>, String> {
    let store = CredentialStore::open().await.str_err()?;
    match store.load(&key_type).await {
        Ok(value) => {
            let masked = if value.len() > 8 {
                format!("{}...{}", &value[..4], &value[value.len() - 4..])
            } else {
                "****".to_string()
            };
            Ok(Some(masked))
        }
        Err(_) => Ok(None),
    }
}
