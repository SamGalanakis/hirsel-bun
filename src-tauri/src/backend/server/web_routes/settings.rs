use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use axum::extract::{Form, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Redirect};
use lash::oauth;
use serde::{Deserialize, Serialize};

use crate::backend::config::LlmProvider;
use crate::backend::credentials::{CodexOAuthCredentials, CredentialStore};
use crate::backend::llm_provider::resolve_provider;
use crate::backend::webui::render_settings_page;

use super::super::AppState;
use super::support::{current_llm_setup_error, patch_elements, provider_name};

#[derive(Deserialize)]
pub struct UpdateProviderForm {
    pub provider: String,
    pub openrouter_base_url: Option<String>,
}

#[derive(Deserialize)]
pub struct StoreApiKeyForm {
    pub api_key: String,
}

#[derive(Deserialize)]
pub struct SaveOpenrouterForm {
    pub api_key: String,
    pub openrouter_base_url: Option<String>,
}

#[derive(Deserialize)]
pub struct CodexStreamQuery {
    pub device_auth_id: String,
    pub user_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDeviceStartResponse {
    pub device_auth_id: String,
    pub user_code: String,
    pub verify_url: String,
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDevicePollRequest {
    pub device_auth_id: String,
    pub user_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDevicePollResponse {
    pub status: String,
    pub authorization_code: Option<String>,
    pub code_verifier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDeviceExchangeRequest {
    pub authorization_code: String,
    pub code_verifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDeviceExchangeResponse {
    pub status: String,
    pub expires_at: u64,
}

pub async fn settings_page(
    State(state): State<Arc<AppState>>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let config = state.config.read().await.clone();
    let store = CredentialStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let openrouter = store
        .load("openrouter_api_key")
        .await
        .ok()
        .map(|value| crate::backend::api_types::mask_credential(&value));
    let tavily = store
        .load("tavily_api_key")
        .await
        .ok()
        .map(|value| crate::backend::api_types::mask_credential(&value));
    let codex_connected = store.load_codex_oauth().await.ok().flatten().is_some();
    let codex_state = query
        .get("device_auth_id")
        .zip(query.get("user_code"))
        .zip(query.get("verify_url"))
        .map(|((device_auth_id, user_code), verify_url)| {
            (
                device_auth_id.as_str(),
                user_code.as_str(),
                verify_url.as_str(),
            )
        });
    let query_requires_setup = query
        .get("required")
        .map(|value| value == "llm")
        .unwrap_or(false);
    let llm_setup_error = current_llm_setup_error(&state).await;
    let setup_required = query_requires_setup || llm_setup_error.is_some();
    let setup_error_owned = query.get("error").cloned().or(llm_setup_error);
    let setup_error = setup_error_owned.as_deref();

    Ok(render_settings_page(
        provider_name(config.llm.provider),
        config.llm.openrouter_base_url.as_deref(),
        openrouter.as_deref(),
        codex_connected,
        tavily.as_deref(),
        codex_state,
        setup_required,
        setup_error,
    ))
}

pub async fn save_llm_settings(
    State(state): State<Arc<AppState>>,
    Form(form): Form<UpdateProviderForm>,
) -> Result<Redirect, (StatusCode, String)> {
    let provider = match form.provider.as_str() {
        "openrouter" => LlmProvider::Openrouter,
        _ => LlmProvider::Codex,
    };

    {
        let mut config = state.config.write().await;
        config.llm.provider = provider;
        config.llm.openrouter_base_url = form
            .openrouter_base_url
            .filter(|value| !value.trim().is_empty());
        config
            .save()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let config = state.config.read().await.clone();
    if resolve_provider(&config).await.is_ok() {
        Ok(Redirect::to("/app"))
    } else {
        Ok(Redirect::to("/app/settings?required=llm"))
    }
}

pub async fn save_openrouter_key(
    State(state): State<Arc<AppState>>,
    Form(form): Form<SaveOpenrouterForm>,
) -> Result<Redirect, (StatusCode, String)> {
    {
        let mut config = state.config.write().await;
        config.llm.provider = LlmProvider::Openrouter;
        config.llm.openrouter_base_url = form
            .openrouter_base_url
            .filter(|value| !value.trim().is_empty());
        config
            .save()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let store = CredentialStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !form.api_key.trim().is_empty() {
        store
            .store_openrouter_api_key(form.api_key.trim())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    let config = state.config.read().await.clone();
    if resolve_provider(&config).await.is_ok() {
        Ok(Redirect::to("/app"))
    } else {
        Ok(Redirect::to("/app/settings?required=llm"))
    }
}

pub async fn save_tavily_key(
    Form(form): Form<StoreApiKeyForm>,
) -> Result<Redirect, (StatusCode, String)> {
    let store = CredentialStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !form.api_key.trim().is_empty() {
        store
            .store("tavily_api_key", form.api_key.trim())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    Ok(Redirect::to("/app/settings"))
}

pub async fn start_codex_device(
    State(state): State<Arc<AppState>>,
) -> Result<Redirect, (StatusCode, String)> {
    {
        let mut config = state.config.write().await;
        config.llm.provider = LlmProvider::Codex;
        config
            .save()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let response = codex_device_start()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Redirect::to(&format!(
        "/app/settings?required=llm&device_auth_id={}&user_code={}&verify_url={}",
        urlencoding::encode(&response.device_auth_id),
        urlencoding::encode(&response.user_code),
        urlencoding::encode(&response.verify_url),
    )))
}

pub async fn stream_codex_device(Query(query): Query<CodexStreamQuery>) -> impl IntoResponse {
    let stream = stream! {
        loop {
            let result = codex_device_poll(CodexDevicePollRequest {
                device_auth_id: query.device_auth_id.clone(),
                user_code: query.user_code.clone(),
            }).await;

            if let Ok(response) = result {
                if response.status == "approved" {
                    if let (Some(authorization_code), Some(code_verifier)) =
                        (response.authorization_code, response.code_verifier)
                    {
                        let _ = codex_device_exchange(CodexDeviceExchangeRequest {
                            authorization_code,
                            code_verifier,
                        })
                        .await;
                        let html = maud::html! {
                            article class="card" id="codex-status" {
                                header {
                                    h3 { (crate::backend::icons::icon("key")) "Codex" }
                                    span class="pill status-working" { "Connected" }
                                }
                                section {
                                    p class="muted" { "Codex OAuth is connected." }
                                }
                            }
                        };
                        yield Ok::<Event, Infallible>(patch_elements("#codex-status", html.into_string()));
                        let header = maud::html! {
                            a href="/app" class="btn ghost" {
                                (crate::backend::icons::icon("arrow-left"))
                                "Open app"
                            }
                        };
                        yield Ok::<Event, Infallible>(patch_elements("#settings-header-action", header.into_string()));
                        yield Ok::<Event, Infallible>(patch_elements(
                            "#settings-setup-alert",
                            r#"<div id="settings-setup-alert"></div>"#.to_string(),
                        ));
                        break;
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    };
    Sse::new(stream)
}

pub async fn codex_device_start() -> Result<CodexDeviceStartResponse, String> {
    let device = oauth::codex_request_device_code()
        .await
        .map_err(|e| format!("Failed to start Codex device auth: {}", e))?;

    Ok(CodexDeviceStartResponse {
        device_auth_id: device.device_auth_id,
        user_code: device.user_code,
        verify_url: oauth::CODEX_DEVICE_VERIFY_URL.to_string(),
        interval: device.interval,
    })
}

pub async fn codex_device_poll(
    body: CodexDevicePollRequest,
) -> Result<CodexDevicePollResponse, String> {
    let polled = oauth::codex_poll_device_auth(&body.device_auth_id, &body.user_code)
        .await
        .map_err(|e| format!("Failed to poll Codex device auth: {}", e))?;

    Ok(match polled {
        Some((authorization_code, code_verifier)) => CodexDevicePollResponse {
            status: "approved".to_string(),
            authorization_code: Some(authorization_code),
            code_verifier: Some(code_verifier),
        },
        None => CodexDevicePollResponse {
            status: "pending".to_string(),
            authorization_code: None,
            code_verifier: None,
        },
    })
}

pub async fn codex_device_exchange(
    body: CodexDeviceExchangeRequest,
) -> Result<CodexDeviceExchangeResponse, String> {
    let tokens = oauth::codex_exchange_code(&body.authorization_code, &body.code_verifier)
        .await
        .map_err(|e| format!("Failed to exchange Codex authorization code: {}", e))?;

    let store = CredentialStore::open()
        .await
        .map_err(|e| format!("Failed to open credential store: {}", e))?;
    store
        .store_codex_oauth(&CodexOAuthCredentials {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_at,
            account_id: tokens.account_id,
        })
        .await
        .map_err(|e| format!("Failed to store Codex tokens: {}", e))?;

    Ok(CodexDeviceExchangeResponse {
        status: "connected".to_string(),
        expires_at: tokens.expires_at,
    })
}
