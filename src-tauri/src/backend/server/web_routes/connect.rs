use std::sync::Arc;

use axum::extract::{Form, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::backend::webui::render_connect_page;

use super::super::AppState;
use super::support::{
    clear_cookie_header, cookie_headers, redirect_with_cookie, sanitize_return_to,
};

#[derive(Deserialize)]
pub struct ConnectQuery {
    pub return_to: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct ConnectForm {
    pub api_key: String,
    pub return_to: Option<String>,
}

#[derive(Deserialize)]
pub struct BootstrapQuery {
    pub api_key: String,
    pub return_to: Option<String>,
}

pub async fn app_root() -> Redirect {
    Redirect::to("/app")
}

pub async fn health() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

pub async fn connect_page(Query(query): Query<ConnectQuery>) -> impl IntoResponse {
    render_connect_page(query.error.as_deref(), query.return_to.as_deref())
}

pub async fn connect_session(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ConnectForm>,
) -> Response {
    if form.api_key != state.api_key {
        let return_to = sanitize_return_to(form.return_to.as_deref());
        return Redirect::to(&format!(
            "/connect?error=Invalid%20API%20key&return_to={}",
            urlencoding::encode(&return_to)
        ))
        .into_response();
    }

    redirect_with_cookie(
        &sanitize_return_to(form.return_to.as_deref()),
        cookie_headers(&form.api_key),
    )
}

pub async fn connect_bootstrap(
    State(state): State<Arc<AppState>>,
    Query(query): Query<BootstrapQuery>,
) -> Response {
    if query.api_key != state.api_key {
        return Redirect::to("/connect?error=Invalid%20API%20key").into_response();
    }

    redirect_with_cookie(
        &sanitize_return_to(query.return_to.as_deref()),
        cookie_headers(&query.api_key),
    )
}

pub async fn connect_logout() -> Response {
    redirect_with_cookie("/connect", clear_cookie_header())
}

pub async fn webui_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../webui.css"),
    )
}

pub async fn datastar_bundle() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../vendor/datastar.js"),
    )
}
