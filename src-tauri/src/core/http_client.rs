//! Shared authenticated HTTP client for remote API access.
//!
//! This module provides:
//! - `AuthenticatedClient`: A wrapper around `reqwest::Client` with bearer token auth
//! - `ResponseExt`: Extension trait for consistent HTTP response handling
//!
//! Used by:
//! - `RemoteOrchestrator` for run management API calls
//! - Shepherd session commands for remote API calls
//! - `DaemonClient` for daemon communication
//! - Backend-side worker services for remote HTTP calls

use async_trait::async_trait;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

/// Errors that can occur during HTTP requests
#[derive(Debug, Error)]
pub enum HttpError {
    /// HTTP response with non-success status code
    #[error("HTTP {status} from {url}: {body}")]
    Response {
        status: u16,
        url: String,
        body: String,
    },

    /// Request failed (network error, timeout, etc.)
    #[error("Request failed: {0}")]
    Request(#[from] reqwest::Error),

    /// Failed to parse response body
    #[error("Parse error: {0}")]
    Parse(String),
}

/// Result type for HTTP operations
pub type HttpResult<T> = Result<T, HttpError>;

// =============================================================================
// Response Extension Trait
// =============================================================================

/// Extension trait for consistent HTTP response handling.
///
/// Provides methods to check success status and deserialize JSON responses
/// with proper error handling. Use this trait to reduce boilerplate when
/// working with `reqwest::Response`.
///
/// # Example
///
/// ```ignore
/// use crate::core::http_client::ResponseExt;
///
/// // Before (6 lines):
/// let response = client.get(url).send().await?;
/// if !response.status().is_success() {
///     let status = response.status();
///     let body = response.text().await.unwrap_or_default();
///     return Err(anyhow!("HTTP {}: {}", status, body));
/// }
/// let result: T = response.json().await?;
///
/// // After (1 line):
/// let result: T = client.get(url).send().await?.json_or_error().await?;
/// ```
#[async_trait]
pub trait ResponseExt {
    /// Check if response is successful, returning error with body on failure.
    ///
    /// Consumes the response if status is not successful (2xx).
    async fn success_or_error(self) -> HttpResult<Self>
    where
        Self: Sized;

    /// Check success and deserialize JSON body in one call.
    ///
    /// Combines `success_or_error()` with JSON deserialization.
    async fn json_or_error<T: DeserializeOwned>(self) -> HttpResult<T>;

    /// Check success and return the text body.
    async fn text_or_error(self) -> HttpResult<String>;

    /// Check success and return raw bytes.
    async fn bytes_or_error(self) -> HttpResult<Vec<u8>>;
}

#[async_trait]
impl ResponseExt for reqwest::Response {
    async fn success_or_error(self) -> HttpResult<Self> {
        if !self.status().is_success() {
            let status = self.status().as_u16();
            let url = self.url().to_string();
            let body = self.text().await.unwrap_or_default();
            return Err(HttpError::Response { status, url, body });
        }
        Ok(self)
    }

    async fn json_or_error<T: DeserializeOwned>(self) -> HttpResult<T> {
        let url = self.url().to_string();
        let response = self.success_or_error().await?;
        response
            .json()
            .await
            .map_err(|e| HttpError::Parse(format!("{}: {}", url, e)))
    }

    async fn text_or_error(self) -> HttpResult<String> {
        let url = self.url().to_string();
        let response = self.success_or_error().await?;
        response
            .text()
            .await
            .map_err(|e| HttpError::Parse(format!("{}: {}", url, e)))
    }

    async fn bytes_or_error(self) -> HttpResult<Vec<u8>> {
        let url = self.url().to_string();
        let response = self.success_or_error().await?;
        let bytes = response
            .bytes()
            .await
            .map_err(|e| HttpError::Parse(format!("{}: {}", url, e)))?;
        Ok(bytes.to_vec())
    }
}

// =============================================================================
// Authenticated Client
// =============================================================================

/// HTTP client with bearer token authentication.
///
/// Provides typed request methods that automatically:
/// - Add `Authorization: Bearer <token>` header
/// - Handle non-success HTTP status codes
/// - Deserialize JSON responses
///
/// # Example
///
/// ```ignore
/// let client = AuthenticatedClient::new("https://api.example.com", "my-api-key");
///
/// // GET request with typed response
/// let runtimes: Vec<Runtime> = client.get("/api/runtimes").await?;
///
/// // POST request with body and typed response
/// let result: CreateResponse = client.post("/api/runtimes", &request).await?;
///
/// // POST with no response body
/// client.post_empty("/api/runtimes/my-runtime/pause", &()).await?;
///
/// // DELETE request
/// client.delete("/api/runtimes/my-runtime").await?;
/// ```
pub struct AuthenticatedClient {
    client: Client,
    base_url: String,
    api_key: String,
}

impl AuthenticatedClient {
    /// Create a new authenticated client.
    ///
    /// The base URL should not have a trailing slash.
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self {
            client: Client::new(),
            base_url,
            api_key: api_key.into(),
        }
    }

    /// Create a new authenticated client with a custom reqwest client.
    ///
    /// Use this when you need specific client configuration (timeouts, etc.).
    pub fn with_client(
        client: Client,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self {
            client,
            base_url,
            api_key: api_key.into(),
        }
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the underlying reqwest client for custom requests (e.g., SSE streaming)
    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// Get the authorization header value
    pub fn auth_header(&self) -> String {
        format!("Bearer {}", self.api_key)
    }

    /// Build a full URL from a path
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Make a GET request and deserialize the JSON response
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> HttpResult<T> {
        self.client
            .get(self.url(path))
            .bearer_auth(&self.api_key)
            .send()
            .await?
            .json_or_error()
            .await
    }

    /// Make a POST request with a JSON body and deserialize the response
    pub async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> HttpResult<T> {
        self.client
            .post(self.url(path))
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?
            .json_or_error()
            .await
    }

    /// Make a POST request with a JSON body, ignoring the response body
    pub async fn post_empty<B: Serialize>(&self, path: &str, body: &B) -> HttpResult<()> {
        self.client
            .post(self.url(path))
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?
            .success_or_error()
            .await?;
        Ok(())
    }

    /// Make a PATCH request with a JSON body and deserialize the response
    pub async fn patch<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> HttpResult<T> {
        self.client
            .patch(self.url(path))
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?
            .json_or_error()
            .await
    }

    /// Make a POST request with raw bytes body
    pub async fn post_bytes(
        &self,
        path: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> HttpResult<()> {
        self.client
            .post(self.url(path))
            .bearer_auth(&self.api_key)
            .header("Content-Type", content_type)
            .body(body)
            .send()
            .await?
            .success_or_error()
            .await?;
        Ok(())
    }

    /// Make a GET request and return raw bytes
    pub async fn get_bytes(&self, path: &str) -> HttpResult<Vec<u8>> {
        self.client
            .get(self.url(path))
            .bearer_auth(&self.api_key)
            .send()
            .await?
            .bytes_or_error()
            .await
    }

    /// Make a DELETE request
    pub async fn delete(&self, path: &str) -> HttpResult<()> {
        self.client
            .delete(self.url(path))
            .bearer_auth(&self.api_key)
            .send()
            .await?
            .success_or_error()
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_building() {
        let client = AuthenticatedClient::new("https://example.com", "test-key");
        assert_eq!(
            client.url("/api/runtimes"),
            "https://example.com/api/runtimes"
        );

        // Test trailing slash handling
        let client = AuthenticatedClient::new("https://example.com/", "test-key");
        assert_eq!(
            client.url("/api/runtimes"),
            "https://example.com/api/runtimes"
        );
    }

    #[test]
    fn test_auth_header() {
        let client = AuthenticatedClient::new("https://example.com", "my-secret-key");
        assert_eq!(client.auth_header(), "Bearer my-secret-key");
    }
}
