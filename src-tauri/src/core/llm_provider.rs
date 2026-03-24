//! Shared lash provider resolution from Hirsel config + credential store.

use lash::provider::Provider;

use crate::core::config::{Config, LlmProvider};
use crate::core::credentials::{CodexOAuthCredentials, CredentialStore};

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

async fn load_codex_oauth(store: &CredentialStore) -> Result<CodexOAuthCredentials, String> {
    if let Some(creds) = store
        .load_codex_oauth()
        .await
        .map_err(|e| format!("failed to load codex credentials: {}", e))?
    {
        return Ok(creds);
    }

    let access_token = std::env::var("CODEX_ACCESS_TOKEN")
        .or_else(|_| std::env::var("OPENAI_ACCESS_TOKEN"))
        .map_err(|_| {
            "Codex OAuth not configured. Login via /api/auth/codex/device/* first".to_string()
        })?;
    let refresh_token = std::env::var("CODEX_REFRESH_TOKEN")
        .or_else(|_| std::env::var("OPENAI_REFRESH_TOKEN"))
        .map_err(|_| "Missing CODEX_REFRESH_TOKEN/OPENAI_REFRESH_TOKEN".to_string())?;
    let expires_at = std::env::var("CODEX_EXPIRES_AT")
        .or_else(|_| std::env::var("OPENAI_EXPIRES_AT"))
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(u64::MAX);
    let account_id = std::env::var("CODEX_ACCOUNT_ID")
        .or_else(|_| std::env::var("OPENAI_ACCOUNT_ID"))
        .ok();

    Ok(CodexOAuthCredentials {
        access_token,
        refresh_token,
        expires_at,
        account_id,
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
