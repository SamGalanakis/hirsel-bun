//! Shared S3 client factory.
//!
//! Provides a centralized way to create S3 clients from configuration,
//! eliminating duplicated client initialization code across modules.

use aws_config::BehaviorVersion;
use aws_sdk_s3::{
    config::{Credentials, Region},
    Client,
};

use super::StorageError;
use crate::backend::config::S3Config;

/// Factory for creating S3 clients from configuration.
///
/// Centralizes S3 client creation logic for `S3FileStorage` and `S3ArchiveStrategy`.
pub struct S3ClientFactory;

impl S3ClientFactory {
    /// Create a new S3 client from configuration.
    ///
    /// # Arguments
    /// * `config` - S3 configuration including bucket, credentials, and endpoint
    ///
    /// # Returns
    /// An S3 client ready for use, or an error if configuration is invalid.
    pub async fn create(config: &S3Config) -> Result<Client, StorageError> {
        if config.bucket.is_empty() {
            return Err(StorageError::Config("S3 bucket name is required".into()));
        }

        // Build AWS config
        let mut aws_config_builder = aws_config::defaults(BehaviorVersion::latest());

        // Set region (default to us-east-1 for compatibility)
        let region = config.region.as_deref().unwrap_or("us-east-1");
        aws_config_builder = aws_config_builder.region(Region::new(region.to_string()));

        // Set credentials if provided
        if let (Some(ref access_key), Some(ref secret_key)) =
            (&config.access_key_id, &config.secret_access_key)
        {
            let credentials = Credentials::new(access_key, secret_key, None, None, "hirsel");
            aws_config_builder = aws_config_builder.credentials_provider(credentials);
        }

        let aws_config = aws_config_builder.load().await;

        // Build S3 client with custom endpoint if provided
        let mut s3_config_builder = aws_sdk_s3::config::Builder::from(&aws_config);

        if let Some(ref endpoint) = config.endpoint {
            s3_config_builder = s3_config_builder
                .endpoint_url(endpoint)
                .force_path_style(true); // Required for MinIO and most S3-compatible services
        }

        Ok(Client::from_conf(s3_config_builder.build()))
    }

    /// Get the bucket name from config, with validation.
    pub fn bucket(config: &S3Config) -> Result<String, StorageError> {
        if config.bucket.is_empty() {
            return Err(StorageError::Config("S3 bucket name is required".into()));
        }
        Ok(config.bucket.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_bucket_rejected() {
        let config = S3Config {
            bucket: String::new(),
            ..Default::default()
        };
        let result = S3ClientFactory::bucket(&config);
        assert!(matches!(result, Err(StorageError::Config(_))));
    }

    #[test]
    fn test_valid_bucket() {
        let config = S3Config {
            bucket: "my-bucket".to_string(),
            ..Default::default()
        };
        let result = S3ClientFactory::bucket(&config);
        assert_eq!(result.unwrap(), "my-bucket");
    }
}
