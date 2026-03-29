//! API key authentication middleware

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};

pub const SESSION_COOKIE: &str = "hirsel_session";

fn extract_cookie(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie_header| {
            cookie_header
                .split(';')
                .filter_map(|pair| {
                    let (name, value) = pair.trim().split_once('=')?;
                    Some((name, value))
                })
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.to_string())
        })
}

fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer ").map(|s| s.to_string()))
        .or_else(|| {
            headers
                .get("X-API-Key")
                .and_then(|value| value.to_str().ok())
                .map(|s| s.to_string())
        })
        .or_else(|| extract_cookie(headers, SESSION_COOKIE))
}

pub fn is_public_path(path: &str) -> bool {
    path == "/" || path == "/health" || path.starts_with("/connect") || path.starts_with("/static/")
}

/// Middleware to validate API key from Authorization header
pub async fn api_key_auth(
    State(expected_key): State<String>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let path = request.uri().path().to_string();

    if is_public_path(&path) {
        return Ok(next.run(request).await);
    }

    let api_key = extract_api_key(request.headers());

    match api_key {
        Some(key) if key == expected_key => Ok(next.run(request).await),
        _ if path.starts_with("/app") => {
            Err(Redirect::to("/connect?return_to=/app").into_response())
        }
        _ => Err(StatusCode::UNAUTHORIZED.into_response()),
    }
}
