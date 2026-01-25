//! Shared authenticated HTTP client for remote API access.
//!
//! This module provides `AuthenticatedClient`, a wrapper around `reqwest::Client`
//! that handles bearer token authentication and provides typed request methods.
//!
//! Used by:
//! - `RemoteOrchestrator` for run management API calls
//! - `RemoteChatOrchestrator` for chat session API calls

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
/// let runs: Vec<Run> = client.get("/api/runs").await?;
///
/// // POST request with body and typed response
/// let result: CreateResponse = client.post("/api/runs", &request).await?;
///
/// // POST with no response body
/// client.post_empty("/api/runs/my-run/pause", &()).await?;
///
/// // DELETE request
/// client.delete("/api/runs/my-run").await?;
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
        let url = self.url(path);

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await?;

        self.handle_response(resp, &url).await
    }

    /// Make a POST request with a JSON body and deserialize the response
    pub async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> HttpResult<T> {
        let url = self.url(path);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?;

        self.handle_response(resp, &url).await
    }

    /// Make a POST request with a JSON body, ignoring the response body
    pub async fn post_empty<B: Serialize>(&self, path: &str, body: &B) -> HttpResult<()> {
        let url = self.url(path);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?;

        self.handle_empty_response(resp, &url).await
    }

    /// Make a PATCH request with a JSON body and deserialize the response
    pub async fn patch<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> HttpResult<T> {
        let url = self.url(path);

        let resp = self
            .client
            .patch(&url)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?;

        self.handle_response(resp, &url).await
    }

    /// Make a POST request with raw bytes body
    pub async fn post_bytes(
        &self,
        path: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> HttpResult<()> {
        let url = self.url(path);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("Content-Type", content_type)
            .body(body)
            .send()
            .await?;

        self.handle_empty_response(resp, &url).await
    }

    /// Make a GET request and return raw bytes
    pub async fn get_bytes(&self, path: &str) -> HttpResult<Vec<u8>> {
        let url = self.url(path);

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(HttpError::Response { status, url, body });
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| HttpError::Parse(format!("Failed to read response: {}", e)))?;

        Ok(bytes.to_vec())
    }

    /// Make a DELETE request
    pub async fn delete(&self, path: &str) -> HttpResult<()> {
        let url = self.url(path);

        let resp = self
            .client
            .delete(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await?;

        self.handle_empty_response(resp, &url).await
    }

    /// Handle a response that should have a JSON body
    async fn handle_response<T: DeserializeOwned>(
        &self,
        resp: reqwest::Response,
        url: &str,
    ) -> HttpResult<T> {
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(HttpError::Response {
                status,
                url: url.to_string(),
                body,
            });
        }

        resp.json()
            .await
            .map_err(|e| HttpError::Parse(format!("JSON parse error: {}", e)))
    }

    /// Handle a response where we only care about success/failure
    async fn handle_empty_response(&self, resp: reqwest::Response, url: &str) -> HttpResult<()> {
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(HttpError::Response {
                status,
                url: url.to_string(),
                body,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_building() {
        let client = AuthenticatedClient::new("https://example.com", "test-key");
        assert_eq!(client.url("/api/runs"), "https://example.com/api/runs");

        // Test trailing slash handling
        let client = AuthenticatedClient::new("https://example.com/", "test-key");
        assert_eq!(client.url("/api/runs"), "https://example.com/api/runs");
    }

    #[test]
    fn test_auth_header() {
        let client = AuthenticatedClient::new("https://example.com", "my-secret-key");
        assert_eq!(client.auth_header(), "Bearer my-secret-key");
    }
}
