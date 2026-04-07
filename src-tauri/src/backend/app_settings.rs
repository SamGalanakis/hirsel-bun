use serde::{Deserialize, Serialize};
#[cfg(feature = "host")]
use surrealdb::types::SurrealValue;

use crate::backend::config::{LlmProvider, RoleModelConfig, RoleModelOverrides};
#[cfg(feature = "host")]
use crate::backend::db::{global_db, utc_now, DbClient};

#[cfg(feature = "host")]
const APP_SETTING_TABLE: &str = "app_setting";
#[cfg(feature = "host")]
const LLM_SETTINGS_KEY: &str = "llm";
const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

#[cfg(feature = "host")]
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct LlmSettingsRecord {
    provider: String,
    openrouter_base_url: Option<String>,
    shepherd_model: Option<String>,
    shepherd_model_variant: Option<String>,
    librarian_model: Option<String>,
    librarian_model_variant: Option<String>,
    thread_model: Option<String>,
    thread_model_variant: Option<String>,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSettings {
    pub provider: LlmProvider,
    pub openrouter_base_url: Option<String>,
    pub role_models: Option<RoleModelOverrides>,
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            provider: LlmProvider::Codex,
            openrouter_base_url: None,
            role_models: None,
        }
    }
}

#[cfg(feature = "host")]
pub struct AppSettingsStore;

#[cfg(feature = "host")]
impl AppSettingsStore {
    pub async fn open() -> Result<Self, surrealdb::Error> {
        let _ = global_db().await;
        Ok(Self)
    }

    async fn db(&self) -> &'static DbClient {
        global_db().await
    }

    pub async fn load_llm_settings(&self) -> Result<LlmSettings, surrealdb::Error> {
        let db = self.db().await;
        let record: Option<LlmSettingsRecord> =
            db.select((APP_SETTING_TABLE, LLM_SETTINGS_KEY)).await?;
        Ok(record
            .map(LlmSettingsRecord::into_settings)
            .unwrap_or_default())
    }

    pub async fn save_llm_settings(&self, settings: &LlmSettings) -> Result<(), surrealdb::Error> {
        let db = self.db().await;
        let _: Option<LlmSettingsRecord> = db
            .upsert((APP_SETTING_TABLE, LLM_SETTINGS_KEY))
            .content(LlmSettingsRecord::from_settings(settings))
            .await?;
        Ok(())
    }
}

impl LlmSettings {
    pub fn normalized_openrouter_base_url(&self) -> String {
        self.openrouter_base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_OPENROUTER_BASE_URL)
            .to_string()
    }

    pub fn role_override(
        &self,
        role: crate::backend::llm_provider::RuntimeModelRole,
    ) -> Option<&RoleModelConfig> {
        let roles = self.role_models.as_ref()?;
        match role {
            crate::backend::llm_provider::RuntimeModelRole::Shepherd => roles.shepherd.as_ref(),
            crate::backend::llm_provider::RuntimeModelRole::Librarian => roles.librarian.as_ref(),
            crate::backend::llm_provider::RuntimeModelRole::Thread => roles.thread.as_ref(),
        }
    }
}

#[cfg(feature = "host")]
impl LlmSettingsRecord {
    fn into_settings(self) -> LlmSettings {
        LlmSettings {
            provider: decode_provider(&self.provider),
            openrouter_base_url: self.openrouter_base_url,
            role_models: assemble_role_models(
                self.shepherd_model,
                self.shepherd_model_variant,
                self.librarian_model,
                self.librarian_model_variant,
                self.thread_model,
                self.thread_model_variant,
            ),
        }
    }

    fn from_settings(settings: &LlmSettings) -> Self {
        let shepherd = settings
            .role_models
            .as_ref()
            .and_then(|roles| roles.shepherd.as_ref());
        let librarian = settings
            .role_models
            .as_ref()
            .and_then(|roles| roles.librarian.as_ref());
        let thread = settings
            .role_models
            .as_ref()
            .and_then(|roles| roles.thread.as_ref());
        Self {
            provider: encode_provider(settings.provider).to_string(),
            openrouter_base_url: settings.openrouter_base_url.clone(),
            shepherd_model: shepherd.and_then(|cfg| cfg.model.clone()),
            shepherd_model_variant: shepherd.and_then(|cfg| cfg.model_variant.clone()),
            librarian_model: librarian.and_then(|cfg| cfg.model.clone()),
            librarian_model_variant: librarian.and_then(|cfg| cfg.model_variant.clone()),
            thread_model: thread.and_then(|cfg| cfg.model.clone()),
            thread_model_variant: thread.and_then(|cfg| cfg.model_variant.clone()),
            updated_at: utc_now(),
        }
    }
}

#[cfg(feature = "host")]
fn assemble_role_models(
    shepherd_model: Option<String>,
    shepherd_model_variant: Option<String>,
    librarian_model: Option<String>,
    librarian_model_variant: Option<String>,
    thread_model: Option<String>,
    thread_model_variant: Option<String>,
) -> Option<RoleModelOverrides> {
    let shepherd = normalize_role_config(shepherd_model, shepherd_model_variant);
    let librarian = normalize_role_config(librarian_model, librarian_model_variant);
    let thread = normalize_role_config(thread_model, thread_model_variant);
    if shepherd.is_none() && librarian.is_none() && thread.is_none() {
        None
    } else {
        Some(RoleModelOverrides {
            shepherd,
            librarian,
            thread,
        })
    }
}

#[cfg(feature = "host")]
fn normalize_role_config(
    model: Option<String>,
    model_variant: Option<String>,
) -> Option<RoleModelConfig> {
    let config = RoleModelConfig {
        model: model.and_then(trim_optional),
        model_variant: model_variant.and_then(trim_optional),
    };
    if config.is_empty() {
        None
    } else {
        Some(config)
    }
}

#[cfg(feature = "host")]
fn trim_optional(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(feature = "host")]
fn encode_provider(provider: LlmProvider) -> &'static str {
    match provider {
        LlmProvider::Codex => "codex",
        LlmProvider::Openrouter => "openrouter",
    }
}

#[cfg(feature = "host")]
fn decode_provider(value: &str) -> LlmProvider {
    match value.trim().to_ascii_lowercase().as_str() {
        "openrouter" => LlmProvider::Openrouter,
        _ => LlmProvider::Codex,
    }
}
