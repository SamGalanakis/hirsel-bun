//! API key authentication middleware

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};

/// Middleware to validate API key from Authorization header
pub async fn api_key_auth(
    State(expected_key): State<String>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Skip auth for health endpoint
    if request.uri().path() == "/health" {
        return Ok(next.run(request).await);
    }

    // Extract API key from either Authorization header or X-API-Key header
    let api_key = request
        .headers()
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer ").map(|s| s.to_string()))
        .or_else(|| {
            request
                .headers()
                .get("X-API-Key")
                .and_then(|value| value.to_str().ok())
                .map(|s| s.to_string())
        });

    match api_key {
        Some(key) if key == expected_key => Ok(next.run(request).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
