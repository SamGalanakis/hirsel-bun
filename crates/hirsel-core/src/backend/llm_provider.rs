//! Shared lash provider resolution from Hirsel config + credential store.

use lash::provider::Provider;

use crate::backend::app_settings::LlmSettings;
use crate::backend::config::{LlmProvider, RoleModelConfig};
use crate::backend::credentials::CodexOAuthCredentials;
use crate::backend::credentials::{resolve_codex_oauth_credentials, CredentialStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeModelRole {
    Shepherd,
    Librarian,
    Thread,
    Search,
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

pub fn provider_metadata(settings: &LlmSettings) -> Provider {
    match settings.provider {
        LlmProvider::Codex => Provider::Codex {
            access_token: String::new(),
            refresh_token: String::new(),
            expires_at: 0,
            account_id: None,
            options: lash::provider::ProviderOptions::default(),
        },
        LlmProvider::Openrouter => Provider::OpenAiGeneric {
            api_key: String::new(),
            base_url: settings.normalized_openrouter_base_url(),
            options: lash::provider::ProviderOptions::default(),
        },
    }
}

fn role_override<'a>(
    settings: &'a LlmSettings,
    role: RuntimeModelRole,
) -> Option<&'a RoleModelConfig> {
    settings.role_override(role)
}

fn default_model_for_role(provider: &Provider, role: RuntimeModelRole) -> (String, Option<String>) {
    let preferred_tier = match role {
        RuntimeModelRole::Search => "low",
        RuntimeModelRole::Shepherd | RuntimeModelRole::Librarian | RuntimeModelRole::Thread => {
            "high"
        }
    };

    if let Some((model, variant)) = provider.default_agent_model(preferred_tier) {
        return (model.to_string(), variant.map(str::to_string));
    }

    let model = provider.default_model().to_string();
    let variant = provider.default_model_variant(&model).map(str::to_string);
    (model, variant)
}

/// Resolve model + variant for a specific Hirsel runtime role.
pub fn resolve_model_for_role(
    settings: &LlmSettings,
    provider: &Provider,
    role: RuntimeModelRole,
) -> (String, Option<String>) {
    let (fallback_model, fallback_variant) = default_model_for_role(provider, role);
    let Some(role_cfg) = role_override(settings, role) else {
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

pub async fn resolve_provider(settings: &LlmSettings) -> Result<Provider, String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        return match settings.provider {
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
                    base_url: settings.normalized_openrouter_base_url(),
                    options: lash::provider::ProviderOptions::default(),
                })
            }
        };
    }

    let store = CredentialStore::open()
        .await
        .map_err(|e| format!("failed to open credential store: {}", e))?;

    match settings.provider {
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
                base_url: settings.normalized_openrouter_base_url(),
                options: lash::provider::ProviderOptions::default(),
            })
        }
    }
}
