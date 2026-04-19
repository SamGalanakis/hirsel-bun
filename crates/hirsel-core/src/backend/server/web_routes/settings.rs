use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use lash::oauth;
use lash::provider::Provider;
use serde::{Deserialize, Serialize};

use crate::backend::app_settings::{AppSettingsStore, LlmSettings};
use crate::backend::config::{LlmProvider, RoleModelConfig, RoleModelOverrides};
use crate::backend::credentials::{
    resolve_codex_oauth_credentials, resolve_github_token, resolve_tavily_api_key, CredentialStore,
};
use crate::backend::llm_provider::{self, RuntimeModelRole};
use crate::backend::shepherd_runtime;

use super::super::AppState;

#[derive(Serialize)]
struct ApiSettingsResponse {
    provider: String,
    openrouter_key_masked: Option<String>,
    openrouter_base_url: Option<String>,
    role_models: ApiRoleModelsResponse,
    model_catalog: ApiLlmModelCatalog,
    codex_configured: bool,
    codex_source: Option<String>,
    github_configured: bool,
    github_token_masked: Option<String>,
    github_source: Option<String>,
    tavily_required: bool,
    tavily_configured: bool,
    tavily_key_masked: Option<String>,
    tavily_source: Option<String>,
    /// True iff an OpenRouter key is currently available (env or store).
    /// Embeddings + hybrid retrieval hard-require this key; the frontend
    /// surfaces a banner when false.
    embeddings_ready: bool,
}

#[derive(Serialize)]
struct ApiRoleModelsResponse {
    shepherd: ApiRoleModelResponse,
    librarian: ApiRoleModelResponse,
    thread: ApiRoleModelResponse,
    search: ApiRoleModelResponse,
}

#[derive(Serialize)]
struct ApiRoleModelResponse {
    configured_model: Option<String>,
    configured_model_variant: Option<String>,
    effective_model: String,
    effective_model_variant: Option<String>,
}

#[derive(Serialize)]
struct ApiLlmModelCatalog {
    model_options: Vec<ApiLlmModelOption>,
    variant_options: BTreeMap<String, Vec<String>>,
    default_variants: BTreeMap<String, Option<String>>,
}

#[derive(Serialize)]
struct ApiLlmModelOption {
    value: String,
    label: String,
    description: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCodexDeviceStartResponse {
    status: String,
    device_auth_id: String,
    user_code: String,
    verify_url: String,
    interval: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCodexDevicePollRequest {
    device_auth_id: String,
    user_code: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCodexDevicePollResponse {
    status: String,
    expires_at: Option<u64>,
}

#[derive(Deserialize)]
pub struct SaveLlmProviderBody {
    provider: String,
}

#[derive(Deserialize, Default)]
pub struct SaveRoleModelsBody {
    #[serde(default)]
    shepherd: SaveRoleModelInput,
    #[serde(default)]
    librarian: SaveRoleModelInput,
    #[serde(default)]
    thread: SaveRoleModelInput,
    #[serde(default)]
    search: SaveRoleModelInput,
}

#[derive(Deserialize, Default)]
pub struct SaveRoleModelInput {
    model: Option<String>,
    model_variant: Option<String>,
}

#[derive(Deserialize)]
pub struct SaveOpenrouterKeyBody {
    api_key: String,
    base_url: Option<String>,
}

#[derive(Deserialize)]
pub struct SaveTavilyKeyBody {
    api_key: String,
}

#[derive(Deserialize)]
pub struct SaveGithubTokenBody {
    token: String,
}

fn trim_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn curated_model_options(provider: &Provider) -> Vec<ApiLlmModelOption> {
    match provider {
        Provider::Codex { .. } => vec![
            ApiLlmModelOption {
                value: "gpt-5.4-mini".to_string(),
                label: "GPT-5.4 Mini".to_string(),
                description: Some("Lash default fast Codex model.".to_string()),
            },
            ApiLlmModelOption {
                value: "gpt-5.4".to_string(),
                label: "GPT-5.4".to_string(),
                description: Some("Lash default main Codex model.".to_string()),
            },
            ApiLlmModelOption {
                value: "gpt-5.3-codex".to_string(),
                label: "GPT-5.3 Codex".to_string(),
                description: Some("Older Codex-tuned model with xhigh variants.".to_string()),
            },
            ApiLlmModelOption {
                value: "gpt-5.1-codex".to_string(),
                label: "GPT-5.1 Codex".to_string(),
                description: Some("Legacy Codex model.".to_string()),
            },
        ],
        Provider::OpenAiGeneric { .. } => vec![
            ApiLlmModelOption {
                value: "minimax/minimax-m2.5".to_string(),
                label: "MiniMax M2.5".to_string(),
                description: Some("Lash default fast OpenRouter model.".to_string()),
            },
            ApiLlmModelOption {
                value: "z-ai/glm-5".to_string(),
                label: "GLM-5".to_string(),
                description: Some("Lash default balanced OpenRouter model.".to_string()),
            },
            ApiLlmModelOption {
                value: "anthropic/claude-sonnet-4.6".to_string(),
                label: "Claude Sonnet 4.6".to_string(),
                description: Some("Lash default high OpenRouter model.".to_string()),
            },
            ApiLlmModelOption {
                value: "gpt-5".to_string(),
                label: "GPT-5".to_string(),
                description: Some("OpenAI model via OpenRouter.".to_string()),
            },
            ApiLlmModelOption {
                value: "gpt-5.4".to_string(),
                label: "GPT-5.4".to_string(),
                description: Some("OpenAI 5.4 via OpenRouter.".to_string()),
            },
        ],
        Provider::GoogleOAuth { .. } => Vec::new(),
    }
}

fn ensure_model_option(
    options: &mut Vec<ApiLlmModelOption>,
    value: &str,
    description: Option<&str>,
) {
    if value.trim().is_empty() || options.iter().any(|option| option.value == value) {
        return;
    }
    options.push(ApiLlmModelOption {
        value: value.to_string(),
        label: value.to_string(),
        description: description.map(str::to_string),
    });
}

fn build_model_catalog(settings: &LlmSettings) -> ApiLlmModelCatalog {
    let provider = llm_provider::provider_metadata(settings);
    let mut model_options = curated_model_options(&provider);

    ensure_model_option(
        &mut model_options,
        provider.default_model(),
        Some("Provider default."),
    );
    for role in [
        RuntimeModelRole::Shepherd,
        RuntimeModelRole::Librarian,
        RuntimeModelRole::Thread,
        RuntimeModelRole::Search,
    ] {
        let (effective_model, _) = llm_provider::resolve_model_for_role(settings, &provider, role);
        ensure_model_option(
            &mut model_options,
            &effective_model,
            Some("Currently active."),
        );
    }

    let mut variant_options = BTreeMap::new();
    let mut default_variants = BTreeMap::new();
    for option in &model_options {
        variant_options.insert(
            option.value.clone(),
            provider
                .supported_variants(&option.value)
                .iter()
                .map(|value| value.to_string())
                .collect(),
        );
        default_variants.insert(
            option.value.clone(),
            provider
                .default_model_variant(&option.value)
                .map(str::to_string),
        );
    }

    ApiLlmModelCatalog {
        model_options,
        variant_options,
        default_variants,
    }
}

fn role_model_response(settings: &LlmSettings, role: RuntimeModelRole) -> ApiRoleModelResponse {
    let provider = llm_provider::provider_metadata(settings);
    let overrides = settings.role_models.as_ref();
    let configured = match role {
        RuntimeModelRole::Shepherd => overrides.and_then(|value| value.shepherd.as_ref()),
        RuntimeModelRole::Librarian => overrides.and_then(|value| value.librarian.as_ref()),
        RuntimeModelRole::Thread => overrides.and_then(|value| value.thread.as_ref()),
        RuntimeModelRole::Search => overrides.and_then(|value| value.search.as_ref()),
    };
    let (effective_model, effective_model_variant) =
        llm_provider::resolve_model_for_role(settings, &provider, role);

    ApiRoleModelResponse {
        configured_model: configured.and_then(|value| value.model.clone()),
        configured_model_variant: configured.and_then(|value| value.model_variant.clone()),
        effective_model,
        effective_model_variant,
    }
}

fn normalize_role_model_input(input: SaveRoleModelInput) -> Option<RoleModelConfig> {
    let config = RoleModelConfig {
        model: trim_optional_string(input.model),
        model_variant: trim_optional_string(input.model_variant)
            .map(|value| value.to_ascii_lowercase()),
    };
    if config.is_empty() {
        None
    } else {
        Some(config)
    }
}

fn schedule_scope_session_reset(reason: &'static str) {
    tokio::spawn(async move {
        if let Err(error) = shepherd_runtime::reset_all_scope_sessions().await {
            tracing::warn!(%error, reason, "failed to reset scope sessions after settings change");
        }
    });
}

pub async fn get_settings(
    State(_state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let settings_store = AppSettingsStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let llm_settings = settings_store
        .load_llm_settings()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let store = CredentialStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    let openrouter_key_masked = store
        .load("openrouter_api_key")
        .await
        .ok()
        .map(|value| crate::backend::api_types::mask_credential(&value));
    let codex = resolve_codex_oauth_credentials().await;
    let github = resolve_github_token().await;
    let tavily = resolve_tavily_api_key().await;
    let embeddings_ready = crate::backend::embeddings::EmbeddingClient::is_configured().await;

    let provider = match llm_settings.provider {
        LlmProvider::Codex => "codex",
        LlmProvider::Openrouter => "openrouter",
    };

    Ok(Json(ApiSettingsResponse {
        provider: provider.to_string(),
        openrouter_key_masked,
        openrouter_base_url: llm_settings.openrouter_base_url.clone(),
        role_models: ApiRoleModelsResponse {
            shepherd: role_model_response(&llm_settings, RuntimeModelRole::Shepherd),
            librarian: role_model_response(&llm_settings, RuntimeModelRole::Librarian),
            thread: role_model_response(&llm_settings, RuntimeModelRole::Thread),
            search: role_model_response(&llm_settings, RuntimeModelRole::Search),
        },
        model_catalog: build_model_catalog(&llm_settings),
        codex_configured: codex.is_some(),
        codex_source: codex.map(|value| value.source.as_str().to_string()),
        github_configured: github.is_some(),
        github_token_masked: github
            .as_ref()
            .map(|value| crate::backend::api_types::mask_credential(&value.token)),
        github_source: github.map(|value| value.source.as_str().to_string()),
        tavily_required: true,
        tavily_configured: tavily.is_some(),
        tavily_key_masked: tavily
            .as_ref()
            .map(|value| crate::backend::api_types::mask_credential(&value.api_key)),
        tavily_source: tavily.map(|value| value.source.as_str().to_string()),
        embeddings_ready,
    }))
}

pub async fn save_llm_provider(
    State(_state): State<Arc<AppState>>,
    Json(body): Json<SaveLlmProviderBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let provider = match body.provider.as_str() {
        "openrouter" => LlmProvider::Openrouter,
        _ => LlmProvider::Codex,
    };

    let settings_store = AppSettingsStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let mut settings = settings_store
        .load_llm_settings()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    settings.provider = provider;
    settings_store
        .save_llm_settings(&settings)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    schedule_scope_session_reset("save_llm_provider");

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn save_role_models(
    State(_state): State<Arc<AppState>>,
    Json(body): Json<SaveRoleModelsBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let settings_store = AppSettingsStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let mut settings = settings_store
        .load_llm_settings()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let role_models = RoleModelOverrides {
        shepherd: normalize_role_model_input(body.shepherd),
        librarian: normalize_role_model_input(body.librarian),
        thread: normalize_role_model_input(body.thread),
        search: normalize_role_model_input(body.search),
    };
    let provider = llm_provider::provider_metadata(&settings);

    for role in [
        RuntimeModelRole::Shepherd,
        RuntimeModelRole::Librarian,
        RuntimeModelRole::Thread,
        RuntimeModelRole::Search,
    ] {
        let role_cfg = match role {
            RuntimeModelRole::Shepherd => role_models.shepherd.as_ref(),
            RuntimeModelRole::Librarian => role_models.librarian.as_ref(),
            RuntimeModelRole::Thread => role_models.thread.as_ref(),
            RuntimeModelRole::Search => role_models.search.as_ref(),
        };
        let Some(role_cfg) = role_cfg else {
            continue;
        };
        if let Some(model) = role_cfg.model.as_deref() {
            provider
                .validate_model_name(model)
                .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
        }
    }

    settings.role_models = if role_models.shepherd.is_none()
        && role_models.librarian.is_none()
        && role_models.thread.is_none()
        && role_models.search.is_none()
    {
        None
    } else {
        Some(role_models.clone())
    };

    for role in [
        RuntimeModelRole::Shepherd,
        RuntimeModelRole::Librarian,
        RuntimeModelRole::Thread,
        RuntimeModelRole::Search,
    ] {
        let role_cfg = match role {
            RuntimeModelRole::Shepherd => role_models.shepherd.as_ref(),
            RuntimeModelRole::Librarian => role_models.librarian.as_ref(),
            RuntimeModelRole::Thread => role_models.thread.as_ref(),
            RuntimeModelRole::Search => role_models.search.as_ref(),
        };
        let Some(role_cfg) = role_cfg else {
            continue;
        };
        if let Some(variant) = role_cfg.model_variant.as_deref() {
            let (effective_model, _) =
                llm_provider::resolve_model_for_role(&settings, &provider, role);
            provider
                .validate_variant(&effective_model, variant)
                .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
        }
    }

    settings_store
        .save_llm_settings(&settings)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    schedule_scope_session_reset("save_role_models");

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn start_codex_device_flow(
    State(_state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let settings_store = AppSettingsStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let mut settings = settings_store
        .load_llm_settings()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    settings.provider = LlmProvider::Codex;
    settings_store
        .save_llm_settings(&settings)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    schedule_scope_session_reset("start_codex_device_flow");

    let device = oauth::codex_request_device_code().await.map_err(|error| {
        (
            StatusCode::BAD_GATEWAY,
            format!("Failed to start Codex device auth: {}", error),
        )
    })?;

    Ok(Json(ApiCodexDeviceStartResponse {
        status: "pending".to_string(),
        device_auth_id: device.device_auth_id,
        user_code: device.user_code,
        verify_url: oauth::CODEX_DEVICE_VERIFY_URL.to_string(),
        interval: device.interval,
    }))
}

pub async fn poll_codex_device_flow(
    Json(body): Json<ApiCodexDevicePollRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let polled = oauth::codex_poll_device_auth(&body.device_auth_id, &body.user_code)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to poll Codex device auth: {}", error),
            )
        })?;

    let Some((authorization_code, code_verifier)) = polled else {
        return Ok(Json(ApiCodexDevicePollResponse {
            status: "pending".to_string(),
            expires_at: None,
        }));
    };

    let tokens = oauth::codex_exchange_code(&authorization_code, &code_verifier)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to exchange Codex auth code: {}", error),
            )
        })?;

    let expires_at = tokens.expires_at;
    let store = CredentialStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    store
        .store_codex_oauth(&crate::backend::credentials::CodexOAuthCredentials {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_at,
            account_id: tokens.account_id,
        })
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    schedule_scope_session_reset("poll_codex_device_flow");

    Ok(Json(ApiCodexDevicePollResponse {
        status: "connected".to_string(),
        expires_at: Some(expires_at),
    }))
}

/// Persist the OpenRouter API key and (optional) base URL.
///
/// Note: the caller controls the active LLM provider via
/// `save_llm_provider`. Saving an OpenRouter key here does NOT flip the
/// provider, so a Codex-native user can configure the key purely for
/// semantic-retrieval embeddings without disturbing their chat setup.
pub async fn save_openrouter_key(
    State(_state): State<Arc<AppState>>,
    Json(body): Json<SaveOpenrouterKeyBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let settings_store = AppSettingsStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let mut settings = settings_store
        .load_llm_settings()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if let Some(base_url) = body.base_url {
        let trimmed = base_url.trim();
        settings.openrouter_base_url = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        settings_store
            .save_llm_settings(&settings)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    }

    let store = CredentialStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if !body.api_key.trim().is_empty() {
        store
            .store_openrouter_api_key(body.api_key.trim())
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    }
    schedule_scope_session_reset("save_openrouter_key");

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn save_tavily_key(
    Json(body): Json<SaveTavilyKeyBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let api_key = body.api_key.trim();
    if api_key.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Tavily is required. Enter a Tavily key or set TAVILY_API_KEY.".to_string(),
        ));
    }

    let store = CredentialStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    store
        .store("tavily_api_key", api_key)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    schedule_scope_session_reset("save_tavily_key");

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn save_github_token(
    Json(body): Json<SaveGithubTokenBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let token = body.token.trim();
    if token.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Enter a GitHub token or set GITHUB_TOKEN / GH_TOKEN in the environment.".to_string(),
        ));
    }

    let store = CredentialStore::open()
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    store
        .store("github_token", token)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    unsafe {
        std::env::set_var("GITHUB_TOKEN", token);
        std::env::set_var("GH_TOKEN", token);
    }
    schedule_scope_session_reset("save_github_token");

    Ok(Json(serde_json::json!({ "ok": true })))
}
