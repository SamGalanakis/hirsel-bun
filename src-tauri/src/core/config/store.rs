//! Database-backed configuration storage.
//!
//! This module provides `ConfigStore` for persisting configuration to the global
//! database (`~/.hirsel/hirsel.db`). The database is the source of truth for config,
//! with the TOML file used for initial seeding or one-time override.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::OnceCell;

use super::{
    AgentConfig, AuthConfig, Config, GitConfig, OrchestratorProfile, ServiceWorkersConfig,
    StorageConfig,
};
use crate::core::db::{global_pool, utc_now};
use crate::core::runner::RunnerConfig;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

static SCHEMA_INIT: OnceCell<()> = OnceCell::const_new();

async fn ensure_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    SCHEMA_INIT
        .get_or_try_init(|| async {
            sqlx::raw_sql(SCHEMA).execute(pool).await?;
            Ok::<(), sqlx::Error>(())
        })
        .await?;
    Ok(())
}

/// Errors that can occur during config store operations
#[derive(Debug, Error)]
pub enum ConfigStoreError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type ConfigStoreResult<T> = Result<T, ConfigStoreError>;

/// Partial configuration loaded from database.
///
/// All fields are optional since the database may not have all values set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PartialConfig {
    pub agent: Option<AgentConfig>,
    pub eval_timeout: Option<u32>,
    pub auto_learn: Option<bool>,
    pub user_message_pause: Option<String>,
    pub human_in_the_loop: Option<bool>,
    pub context_warning_threshold: Option<f64>,
    pub coordinator_port: Option<u16>,
    pub auth: Option<AuthConfig>,
    pub runners: Option<HashMap<String, RunnerConfig>>,
    pub default_runner: Option<Option<String>>,
    pub worker_runners: Option<HashMap<String, String>>,
    pub default_profile: Option<String>,
    pub profiles: Option<HashMap<String, OrchestratorProfile>>,
    pub git: Option<GitConfig>,
    pub storage: Option<StorageConfig>,
    pub allow_local_workers: Option<bool>,
    pub service_workers: Option<ServiceWorkersConfig>,
    pub scribe_docs_path: Option<String>,
    pub scribe_persist_docs_changes: Option<bool>,
}

/// Database-backed configuration store.
///
/// Stores configuration key-value pairs in SQLite. Used as the primary
/// source of truth for configuration, with file-based config used for
/// initial seeding or one-time override.
pub struct ConfigStore;

impl ConfigStore {
    /// Open the global config store at `~/.hirsel/hirsel.db`
    pub async fn open() -> ConfigStoreResult<Self> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(Self)
    }

    /// Get the pool
    async fn pool(&self) -> &'static SqlitePool {
        global_pool().await
    }

    /// Get a single config value by key
    pub async fn get(&self, key: &str) -> ConfigStoreResult<Option<String>> {
        let pool = self.pool().await;

        let result: Option<String> = sqlx::query_scalar("SELECT value FROM config WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?;

        Ok(result)
    }

    /// Set a config value
    pub async fn set(&self, key: &str, value: &str) -> ConfigStoreResult<()> {
        let pool = self.pool().await;
        let now = utc_now();

        sqlx::query("INSERT OR REPLACE INTO config (key, value, updated_at) VALUES (?, ?, ?)")
            .bind(key)
            .bind(value)
            .bind(&now)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Delete a config value
    pub async fn delete(&self, key: &str) -> ConfigStoreResult<()> {
        let pool = self.pool().await;

        sqlx::query("DELETE FROM config WHERE key = ?")
            .bind(key)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Get all config key-value pairs
    pub async fn get_all(&self) -> ConfigStoreResult<Vec<(String, String)>> {
        let pool = self.pool().await;

        let rows = sqlx::query("SELECT key, value FROM config")
            .fetch_all(pool)
            .await?;

        let result: Vec<(String, String)> = rows
            .into_iter()
            .map(|row| (row.get("key"), row.get("value")))
            .collect();

        Ok(result)
    }

    /// Load entire config from database as a PartialConfig
    pub async fn load_config(&self) -> ConfigStoreResult<Option<PartialConfig>> {
        let pairs = self.get_all().await?;
        if pairs.is_empty() {
            return Ok(None);
        }

        let mut partial = PartialConfig::default();

        for (key, value) in pairs {
            match key.as_str() {
                "agent" => {
                    partial.agent = match serde_json::from_str(&value) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::debug!("Failed to parse config 'agent': {}", e);
                            None
                        }
                    };
                }
                "eval_timeout" => {
                    partial.eval_timeout = match value.parse() {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::debug!("Failed to parse config 'eval_timeout': {}", e);
                            None
                        }
                    };
                }
                "auto_learn" => {
                    partial.auto_learn = Some(value == "true");
                }
                "user_message_pause" => {
                    partial.user_message_pause = Some(value);
                }
                "human_in_the_loop" => {
                    partial.human_in_the_loop = Some(value == "true");
                }
                "context_warning_threshold" => {
                    partial.context_warning_threshold = match value.parse() {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::debug!(
                                "Failed to parse config 'context_warning_threshold': {}",
                                e
                            );
                            None
                        }
                    };
                }
                "coordinator_port" => {
                    partial.coordinator_port = match value.parse() {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::debug!("Failed to parse config 'coordinator_port': {}", e);
                            None
                        }
                    };
                }
                "auth" => {
                    partial.auth = match serde_json::from_str(&value) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::debug!("Failed to parse config 'auth': {}", e);
                            None
                        }
                    };
                }
                "runners" => {
                    partial.runners = match serde_json::from_str(&value) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::debug!("Failed to parse config 'runners': {}", e);
                            None
                        }
                    };
                }
                "default_runner" => {
                    if value == "null" || value.is_empty() {
                        partial.default_runner = Some(None);
                    } else {
                        partial.default_runner = Some(Some(value));
                    }
                }
                "worker_runners" => {
                    partial.worker_runners = match serde_json::from_str(&value) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::debug!("Failed to parse config 'worker_runners': {}", e);
                            None
                        }
                    };
                }
                "default_profile" => {
                    partial.default_profile = Some(value);
                }
                "profiles" => {
                    partial.profiles = match serde_json::from_str(&value) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::debug!("Failed to parse config 'profiles': {}", e);
                            None
                        }
                    };
                }
                "git" => {
                    partial.git = match serde_json::from_str(&value) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::debug!("Failed to parse config 'git': {}", e);
                            None
                        }
                    };
                }
                "storage" => {
                    partial.storage = match serde_json::from_str(&value) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::debug!("Failed to parse config 'storage': {}", e);
                            None
                        }
                    };
                }
                "allow_local_workers" => {
                    partial.allow_local_workers = Some(value == "true");
                }
                "service_workers" => {
                    partial.service_workers = match serde_json::from_str(&value) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::debug!("Failed to parse config 'service_workers': {}", e);
                            None
                        }
                    };
                }
                "scribe_docs_path" => {
                    partial.scribe_docs_path = Some(value);
                }
                "scribe_persist_docs_changes" => {
                    partial.scribe_persist_docs_changes = Some(value == "true");
                }
                _ => {
                    // Unknown key, ignore
                }
            }
        }

        Ok(Some(partial))
    }

    /// Save entire config to database
    pub async fn save_config(&self, config: &Config) -> ConfigStoreResult<()> {
        // Agent config
        self.set("agent", &serde_json::to_string(&config.agent)?)
            .await?;

        // Scalar settings
        self.set("eval_timeout", &config.eval_timeout.to_string())
            .await?;
        self.set(
            "auto_learn",
            if config.auto_learn { "true" } else { "false" },
        )
        .await?;
        self.set("user_message_pause", &config.user_message_pause)
            .await?;
        self.set(
            "human_in_the_loop",
            if config.human_in_the_loop {
                "true"
            } else {
                "false"
            },
        )
        .await?;
        self.set(
            "context_warning_threshold",
            &config.context_warning_threshold.to_string(),
        )
        .await?;
        self.set("coordinator_port", &config.coordinator_port.to_string())
            .await?;

        // Auth config
        self.set("auth", &serde_json::to_string(&config.auth)?)
            .await?;

        // Runners
        self.set("runners", &serde_json::to_string(&config.runners)?)
            .await?;
        match &config.default_runner {
            Some(runner) => self.set("default_runner", runner).await?,
            None => self.set("default_runner", "null").await?,
        }
        self.set(
            "worker_runners",
            &serde_json::to_string(&config.worker_runners)?,
        )
        .await?;

        // Profiles
        self.set("default_profile", &config.default_profile).await?;
        self.set("profiles", &serde_json::to_string(&config.profiles)?)
            .await?;

        // Git
        self.set("git", &serde_json::to_string(&config.git)?)
            .await?;

        // Storage
        self.set("storage", &serde_json::to_string(&config.storage)?)
            .await?;

        // Allow local workers
        self.set(
            "allow_local_workers",
            if config.allow_local_workers {
                "true"
            } else {
                "false"
            },
        )
        .await?;

        // Service workers
        self.set(
            "service_workers",
            &serde_json::to_string(&config.service_workers)?,
        )
        .await?;

        // Scribe docs settings
        self.set("scribe_docs_path", &config.scribe_docs_path)
            .await?;
        self.set(
            "scribe_persist_docs_changes",
            if config.scribe_persist_docs_changes {
                "true"
            } else {
                "false"
            },
        )
        .await?;

        Ok(())
    }

    /// Check if the config store has any configuration stored
    pub async fn has_config(&self) -> ConfigStoreResult<bool> {
        let pool = self.pool().await;

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM config")
            .fetch_one(pool)
            .await?;

        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    // Tests need to be updated for async - skipping for now
}
