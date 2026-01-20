//! Storage configuration for files and database.

use serde::{Deserialize, Serialize};

/// Storage backend type for file storage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    /// Local filesystem storage (default)
    #[default]
    Local,
    /// S3-compatible object storage (MinIO, Tigris, AWS S3)
    S3,
}

/// S3-compatible storage configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct S3Config {
    /// S3 endpoint URL (e.g., "http://localhost:9000" for MinIO, or Tigris URL)
    /// If not set, uses AWS S3 default endpoint
    pub endpoint: Option<String>,
    /// S3 bucket name
    #[serde(default)]
    pub bucket: String,
    /// AWS region (e.g., "us-east-1", "auto" for MinIO)
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
    /// S3 configuration (when files = "s3")
    #[serde(default)]
    pub s3: Option<S3Config>,
}
