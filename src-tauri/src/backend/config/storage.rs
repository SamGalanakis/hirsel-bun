//! Storage configuration for files and database.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Storage backend type for file storage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    /// Local filesystem storage (default)
    #[default]
    Local,
    /// S3-compatible object storage
    S3,
}

/// Storage provider type (for UI presets)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StorageProvider {
    /// AWS S3
    #[default]
    S3,
    /// MinIO or other self-hosted S3-compatible storage
    Minio,
}

impl StorageProvider {
    /// Get the default endpoint for this provider
    pub fn default_endpoint(&self) -> Option<&'static str> {
        match self {
            StorageProvider::S3 => None, // AWS S3 uses default
            StorageProvider::Minio => Some("http://localhost:9000"),
        }
    }

    /// Get the default region for this provider
    pub fn default_region(&self) -> &'static str {
        match self {
            StorageProvider::S3 => "us-east-1",
            StorageProvider::Minio => "us-east-1",
        }
    }
}

/// S3-compatible storage configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct S3Config {
    /// Provider type (for UI display, doesn't affect functionality)
    #[serde(default)]
    pub provider: StorageProvider,
    /// S3 endpoint URL
    /// If not set, uses AWS S3 default endpoint
    pub endpoint: Option<String>,
    /// S3 bucket name
    #[serde(default)]
    pub bucket: String,
    /// AWS region (for example, "us-east-1")
    #[serde(default)]
    pub region: Option<String>,
    /// AWS access key ID (can also be set via environment)
    pub access_key_id: Option<String>,
    /// AWS secret access key (can also be set via environment)
    pub secret_access_key: Option<String>,
}

/// Storage configuration for files and database
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageConfig {
    /// File storage backend: "local" or "s3"
    #[serde(default)]
    pub files: StorageBackend,
    /// Named storage configurations
    #[serde(default)]
    pub storages: HashMap<String, S3Config>,
    /// Default storage name for snapshots (if not specified per-runner)
    #[serde(default)]
    pub default_storage: Option<String>,
}

impl StorageConfig {
    /// Get a storage config by name, or the default storage
    pub fn get_storage(&self, name: Option<&str>) -> Option<&S3Config> {
        // If name specified, look it up
        if let Some(n) = name {
            if let Some(config) = self.storages.get(n) {
                return Some(config);
            }
        }

        // Try default storage
        if let Some(ref default_name) = self.default_storage {
            return self.storages.get(default_name);
        }

        None
    }

    /// Get all storage names
    pub fn storage_names(&self) -> Vec<String> {
        self.storages.keys().cloned().collect()
    }
}
