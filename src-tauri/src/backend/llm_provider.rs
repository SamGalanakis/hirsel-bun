//! Shared lash provider resolution from Hirsel config + credential store.

use lash::provider::Provider;

use crate::backend::config::{Config, LlmProvider};
use crate::backend::credentials::{resolve_codex_oauth_credentials, CredentialStore};

const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

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

async fn load_openrouter_key(store: &CredentialStore) -> Result<String, String> {
    if let Some(key) = store.load_openrouter_api_key().await.ok().flatten() {
        return Ok(key);
    }
    std::env::var("OPENROUTER_API_KEY").map_err(|_| {
        "OpenRouter API key not configured. Set it in Settings or OPENROUTER_API_KEY".to_string()
    })
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
