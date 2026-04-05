//! Shared lash provider resolution from Hirsel config + credential store.

use lash::provider::Provider;

use crate::backend::config::{Config, LlmProvider, RoleModelConfig};
use crate::backend::credentials::{
    resolve_codex_oauth_credentials, CodexOAuthCredentials, CredentialStore,
};

const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeModelRole {
    Shepherd,
    Librarian,
    Thread,
}

fn normalize_openrouter_base_url(config: &Config) -> String {
    config
        .llm
        .openrouter_base_url
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(DEFAULT_OPENROUTER_BASE_URL)
        .to_string()
}

async fn load_codex_oauth(
    _store: &CredentialStore,
) -> Result<crate::backend::credentials::CodexOAuthCredentials, String> {
    resolve_codex_oauth_credentials()
        .await
        .map(|resolved| resolved.credentials)
        .ok_or_else(|| {
            "Codex credentials not configured. Connect Codex in Settings, or set CODEX_ACCESS_TOKEN and CODEX_REFRESH_TOKEN."
                .to_string()
        })
}

fn load_codex_oauth_from_env() -> Result<CodexOAuthCredentials, String> {
    let access_token = std::env::var("CODEX_ACCESS_TOKEN").map_err(|_| {
        "Codex credentials not configured. Connect Codex in Settings, or set CODEX_ACCESS_TOKEN and CODEX_REFRESH_TOKEN."
            .to_string()
    })?;
    let refresh_token = std::env::var("CODEX_REFRESH_TOKEN").map_err(|_| {
        "Codex credentials not configured. Connect Codex in Settings, or set CODEX_ACCESS_TOKEN and CODEX_REFRESH_TOKEN."
            .to_string()
    })?;
    let expires_at = std::env::var("CODEX_EXPIRES_AT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let account_id = std::env::var("CODEX_ACCOUNT_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    Ok(CodexOAuthCredentials {
        access_token,
        refresh_token,
        expires_at,
        account_id,
    })
}

fn load_openrouter_key_from_env() -> Option<String> {
    std::env::var("OPENROUTER_API_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn load_openrouter_key(store: &CredentialStore) -> Result<String, String> {
    if let Some(key) = load_openrouter_key_from_env() {
        return Ok(key);
    }
    if let Some(key) = store.load_openrouter_api_key().await.ok().flatten() {
        return Ok(key);
    }
    Err("OpenRouter API key not configured. Set it in Settings or OPENROUTER_API_KEY".to_string())
}

pub fn provider_metadata(config: &Config) -> Provider {
    match config.llm.provider {
        LlmProvider::Codex => Provider::Codex {
            access_token: String::new(),
            refresh_token: String::new(),
            expires_at: 0,
            account_id: None,
            options: lash::provider::ProviderOptions::default(),
        },
        LlmProvider::Openrouter => Provider::OpenAiGeneric {
            api_key: String::new(),
            base_url: normalize_openrouter_base_url(config),
            options: lash::provider::ProviderOptions::default(),
        },
    }
}

fn role_override<'a>(config: &'a Config, role: RuntimeModelRole) -> Option<&'a RoleModelConfig> {
    let roles = config.llm.role_models.as_ref()?;
    match role {
        RuntimeModelRole::Shepherd => roles.shepherd.as_ref(),
        RuntimeModelRole::Librarian => roles.librarian.as_ref(),
        RuntimeModelRole::Thread => roles.thread.as_ref(),
    }
}

/// Resolve the model and variant from config, falling back to provider defaults.
pub fn resolve_model(config: &Config, provider: &Provider) -> (String, Option<String>) {
    // If user explicitly configured a model, use it
    if let Some(ref model) = config.llm.model {
        let variant = config
            .llm
            .model_variant
            .clone()
            .or_else(|| provider.default_model_variant(model).map(str::to_string));
        return (model.clone(), variant);
    }

    // Fall back to provider's high-tier default (Shepherd = high intelligence)
    if let Some((m, variant)) = provider.default_agent_model("high") {
        return (m.to_string(), variant.map(str::to_string));
    }

    // Last resort: provider's default model
    let model = provider.default_model().to_string();
    let variant = config
        .llm
        .model_variant
        .clone()
        .or_else(|| provider.default_model_variant(&model).map(str::to_string));
    (model, variant)
}

/// Resolve model + variant for a specific Hirsel runtime role, falling back to
/// the legacy global model config and then the provider defaults.
pub fn resolve_model_for_role(
    config: &Config,
    provider: &Provider,
    role: RuntimeModelRole,
) -> (String, Option<String>) {
    let (fallback_model, fallback_variant) = resolve_model(config, provider);
    let Some(role_cfg) = role_override(config, role) else {
        return (fallback_model, fallback_variant);
    };

    let model = role_cfg
        .model
        .clone()
        .unwrap_or_else(|| fallback_model.clone());

    let variant = role_cfg.model_variant.clone().or_else(|| {
        if role_cfg.model.is_some() {
            provider.default_model_variant(&model).map(str::to_string)
        } else {
            fallback_variant
                .clone()
                .or_else(|| provider.default_model_variant(&model).map(str::to_string))
        }
    });

    (model, variant)
}

/// Resolve the provider's named intelligence tier first, then fall back to the
/// normal model resolution path.
pub fn resolve_model_for_tier(
    config: &Config,
    provider: &Provider,
    tier: &str,
) -> (String, Option<String>) {
    if let Some((model, variant)) = provider.default_agent_model(tier) {
        return (model.to_string(), variant.map(str::to_string));
    }
    resolve_model(config, provider)
}

pub async fn resolve_provider(config: &Config) -> Result<Provider, String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        return match config.llm.provider {
            LlmProvider::Codex => {
                let codex = load_codex_oauth_from_env()?;
                Ok(Provider::Codex {
                    access_token: codex.access_token,
                    refresh_token: codex.refresh_token,
                    expires_at: codex.expires_at,
                    account_id: codex.account_id,
                    options: lash::provider::ProviderOptions::default(),
                })
            }
            LlmProvider::Openrouter => {
                let api_key = load_openrouter_key_from_env().ok_or_else(|| {
                    "OpenRouter API key not configured. Set it in Settings or OPENROUTER_API_KEY"
                        .to_string()
                })?;
                Ok(Provider::OpenAiGeneric {
                    api_key,
                    base_url: normalize_openrouter_base_url(config),
                    options: lash::provider::ProviderOptions::default(),
                })
            }
        };
    }

    let store = CredentialStore::open()
        .await
        .map_err(|e| format!("failed to open credential store: {}", e))?;

    match config.llm.provider {
        LlmProvider::Codex => {
            let codex = load_codex_oauth(&store).await?;
            Ok(Provider::Codex {
                access_token: codex.access_token,
                refresh_token: codex.refresh_token,
                expires_at: codex.expires_at,
                account_id: codex.account_id,
                options: lash::provider::ProviderOptions::default(),
            })
        }
        LlmProvider::Openrouter => {
            let api_key = load_openrouter_key(&store).await?;
            Ok(Provider::OpenAiGeneric {
                api_key,
                base_url: normalize_openrouter_base_url(config),
                options: lash::provider::ProviderOptions::default(),
            })
        }
    }
}
