use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use super::super::AppState;

pub async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

fn cookie_headers(value: &str) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{}={}; HttpOnly; Path=/; SameSite=Lax",
        crate::backend::server::auth::SESSION_COOKIE,
        value
    ))
    .expect("valid cookie header")
}

fn clear_cookie_header() -> HeaderValue {
    HeaderValue::from_static("hirsel_session=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax")
}

#[derive(Deserialize)]
pub struct ConnectBody {
    api_key: String,
}

pub async fn connect(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ConnectBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if state.api_key.trim().is_empty() {
        let cookie = clear_cookie_header();
        let mut response = Json(serde_json::json!({ "ok": true })).into_response();
        response
            .headers_mut()
            .insert(axum::http::header::SET_COOKIE, cookie);
        return Ok(response);
    }

    if body.api_key != state.api_key {
        return Err((StatusCode::UNAUTHORIZED, "Invalid API key".to_string()));
    }

    let cookie = cookie_headers(&body.api_key);
    let mut response = Json(serde_json::json!({ "ok": true })).into_response();
    response
        .headers_mut()
        .insert(axum::http::header::SET_COOKIE, cookie);
    Ok(response)
}

pub async fn logout() -> Result<impl IntoResponse, (StatusCode, String)> {
    let cookie = clear_cookie_header();
    let mut response = Json(serde_json::json!({ "ok": true })).into_response();
    response
        .headers_mut()
        .insert(axum::http::header::SET_COOKIE, cookie);
    Ok(response)
}
