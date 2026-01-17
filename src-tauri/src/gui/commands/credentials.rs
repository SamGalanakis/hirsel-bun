//! Credential management commands
//!
//! Commands for storing and managing encrypted credentials.

use crate::core::credentials::CredentialStore;

/// Store a credential in the encrypted credential store
///
/// This is used for storing API keys that should not be stored
/// in plain text in the config file.
#[tauri::command]
pub fn store_credential(key_type: String, value: String) -> Result<(), String> {
    let store = CredentialStore::open().map_err(|e| e.to_string())?;
    store.store(&key_type, &value).map_err(|e| e.to_string())
}

/// Delete a credential from the credential store
#[tauri::command]
pub fn delete_credential(key_type: String) -> Result<(), String> {
    let store = CredentialStore::open().map_err(|e| e.to_string())?;
    store.delete(&key_type).map_err(|e| e.to_string())
}

/// Check if a credential exists in the credential store
#[tauri::command]
pub fn has_credential(key_type: String) -> Result<bool, String> {
    let store = CredentialStore::open().map_err(|e| e.to_string())?;
    Ok(store.load(&key_type).is_ok())
}

/// Get the masked value of a credential (for display purposes)
///
/// Returns the first 4 and last 4 characters of the credential,
/// or "****" if the credential is short.
#[tauri::command]
pub fn get_credential_masked(key_type: String) -> Result<Option<String>, String> {
    let store = CredentialStore::open().map_err(|e| e.to_string())?;
    match store.load(&key_type) {
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
