//! Database-backed configuration storage.
//!
//! This module provides `ConfigStore` for persisting configuration to the global
//! database (`~/.hirsel/hirsel.db`). The database is the source of truth for config,
//! with the TOML file used for initial seeding or one-time override.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

use super::{AgentConfig, AuthConfig, Config, GitConfig, OrchestratorProfile, StorageConfig};
use crate::core::runner::RunnerConfig;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

/// Errors that can occur during config store operations
#[derive(Debug, Error)]
pub enum ConfigStoreError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

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
    pub max_iterations: Option<Option<u32>>,
    pub user_message_pause: Option<String>,
    pub human_in_the_loop: Option<bool>,
    pub compaction_enabled: Option<bool>,
    pub compaction_threshold: Option<Option<u32>>,
    pub compaction_keep_messages: Option<u32>,
    pub auto_improve: Option<bool>,
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
}

/// Database-backed configuration store.
///
/// Stores configuration key-value pairs in SQLite. Used as the primary
/// source of truth for configuration, with file-based config used for
/// initial seeding or one-time override.
pub struct ConfigStore {
    db: Connection,
}

impl ConfigStore {
    /// Open the global config store at `~/.hirsel/hirsel.db`
    pub fn open() -> ConfigStoreResult<Self> {
        Self::open_at(&super::paths::global_db_path())
    }

    /// Open a config store at a specific path
    pub fn open_at(path: &Path) -> ConfigStoreResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        let store = Self { db };
        store.init_db()?;
        Ok(store)
    }

    fn init_db(&self) -> ConfigStoreResult<()> {
        self.db.execute_batch(SCHEMA)?;
        Ok(())
    }

    /// Get a single config value by key
    pub fn get(&self, key: &str) -> ConfigStoreResult<Option<String>> {
        let mut stmt = self.db.prepare("SELECT value FROM config WHERE key = ?1")?;

        match stmt.query_row(params![key], |row| row.get(0)) {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Set a config value
    pub fn set(&self, key: &str, value: &str) -> ConfigStoreResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT OR REPLACE INTO config (key, value, updated_at) VALUES (?1, ?2, ?3)",
            params![key, value, now],
        )?;
        Ok(())
    }

    /// Delete a config value
    pub fn delete(&self, key: &str) -> ConfigStoreResult<()> {
        self.db
            .execute("DELETE FROM config WHERE key = ?1", params![key])?;
        Ok(())
    }

    /// Get all config key-value pairs
    pub fn get_all(&self) -> ConfigStoreResult<Vec<(String, String)>> {
        let mut stmt = self.db.prepare("SELECT key, value FROM config")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Load entire config from database as a PartialConfig
    pub fn load_config(&self) -> ConfigStoreResult<Option<PartialConfig>> {
        let pairs = self.get_all()?;
        if pairs.is_empty() {
            return Ok(None);
        }

        let mut partial = PartialConfig::default();

        for (key, value) in pairs {
            match key.as_str() {
                "agent" => {
                    partial.agent = serde_json::from_str(&value).ok();
                }
                "eval_timeout" => {
                    partial.eval_timeout = value.parse().ok();
                }
                "auto_learn" => {
                    partial.auto_learn = Some(value == "true");
                }
                "max_iterations" => {
                    if value == "null" {
                        partial.max_iterations = Some(None);
                    } else {
                        partial.max_iterations = value.parse().ok().map(Some);
                    }
                }
                "user_message_pause" => {
                    partial.user_message_pause = Some(value);
                }
                "human_in_the_loop" => {
                    partial.human_in_the_loop = Some(value == "true");
                }
                "compaction_enabled" => {
                    partial.compaction_enabled = Some(value == "true");
                }
                "compaction_threshold" => {
                    if value == "null" {
                        partial.compaction_threshold = Some(None);
                    } else {
                        partial.compaction_threshold = value.parse().ok().map(Some);
                    }
                }
                "compaction_keep_messages" => {
                    partial.compaction_keep_messages = value.parse().ok();
                }
                "auto_improve" => {
                    partial.auto_improve = Some(value == "true");
                }
                "context_warning_threshold" => {
                    partial.context_warning_threshold = value.parse().ok();
                }
                "coordinator_port" => {
                    partial.coordinator_port = value.parse().ok();
                }
                "auth" => {
                    partial.auth = serde_json::from_str(&value).ok();
                }
                "runners" => {
                    partial.runners = serde_json::from_str(&value).ok();
                }
                "default_runner" => {
                    if value == "null" || value.is_empty() {
                        partial.default_runner = Some(None);
                    } else {
                        partial.default_runner = Some(Some(value));
                    }
                }
                "worker_runners" => {
                    partial.worker_runners = serde_json::from_str(&value).ok();
                }
                "default_profile" => {
                    partial.default_profile = Some(value);
                }
                "profiles" => {
                    partial.profiles = serde_json::from_str(&value).ok();
                }
                "git" => {
                    partial.git = serde_json::from_str(&value).ok();
                }
                "storage" => {
                    partial.storage = serde_json::from_str(&value).ok();
                }
                "allow_local_workers" => {
                    partial.allow_local_workers = Some(value == "true");
                }
                _ => {
                    // Unknown key, ignore
                }
            }
        }

        Ok(Some(partial))
    }

    /// Save entire config to database
    pub fn save_config(&self, config: &Config) -> ConfigStoreResult<()> {
        // Agent config
        self.set("agent", &serde_json::to_string(&config.agent)?)?;

        // Scalar settings
        self.set("eval_timeout", &config.eval_timeout.to_string())?;
        self.set(
            "auto_learn",
            if config.auto_learn { "true" } else { "false" },
        )?;
        match config.max_iterations {
            Some(n) => self.set("max_iterations", &n.to_string())?,
            None => self.set("max_iterations", "null")?,
        }
        self.set("user_message_pause", &config.user_message_pause)?;
        self.set(
            "human_in_the_loop",
            if config.human_in_the_loop {
                "true"
            } else {
                "false"
            },
        )?;
        self.set(
            "compaction_enabled",
            if config.compaction_enabled {
                "true"
            } else {
                "false"
            },
        )?;
        match config.compaction_threshold {
            Some(n) => self.set("compaction_threshold", &n.to_string())?,
            None => self.set("compaction_threshold", "null")?,
        }
        self.set(
            "compaction_keep_messages",
            &config.compaction_keep_messages.to_string(),
        )?;
        self.set(
            "auto_improve",
            if config.auto_improve { "true" } else { "false" },
        )?;
        self.set(
            "context_warning_threshold",
            &config.context_warning_threshold.to_string(),
        )?;
        self.set("coordinator_port", &config.coordinator_port.to_string())?;

        // Auth config
        self.set("auth", &serde_json::to_string(&config.auth)?)?;

        // Runners
        self.set("runners", &serde_json::to_string(&config.runners)?)?;
        match &config.default_runner {
            Some(runner) => self.set("default_runner", runner)?,
            None => self.set("default_runner", "null")?,
        }
        self.set(
            "worker_runners",
            &serde_json::to_string(&config.worker_runners)?,
        )?;

        // Profiles
        self.set("default_profile", &config.default_profile)?;
        self.set("profiles", &serde_json::to_string(&config.profiles)?)?;

        // Git
        self.set("git", &serde_json::to_string(&config.git)?)?;

        // Storage
        self.set("storage", &serde_json::to_string(&config.storage)?)?;

        // Allow local workers
        self.set(
            "allow_local_workers",
            if config.allow_local_workers {
                "true"
            } else {
                "false"
            },
        )?;

        Ok(())
    }

    /// Check if the config store has any configuration stored
    pub fn has_config(&self) -> ConfigStoreResult<bool> {
        let count: i64 = self
            .db
            .query_row("SELECT COUNT(*) FROM config", [], |row| row.get(0))?;
        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_env() -> (TempDir, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test.db");
        (tmp, db_path)
    }

    #[test]
    fn test_get_set_delete() {
        let (_tmp, db_path) = setup_test_env();
        let store = ConfigStore::open_at(&db_path).unwrap();

        // Set a value
        store.set("test_key", "test_value").unwrap();

        // Get it back
        let value = store.get("test_key").unwrap();
        assert_eq!(value, Some("test_value".to_string()));

        // Delete it
        store.delete("test_key").unwrap();

        // Should be gone
        let value = store.get("test_key").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_get_nonexistent() {
        let (_tmp, db_path) = setup_test_env();
        let store = ConfigStore::open_at(&db_path).unwrap();

        let value = store.get("nonexistent").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_get_all() {
        let (_tmp, db_path) = setup_test_env();
        let store = ConfigStore::open_at(&db_path).unwrap();

        store.set("key1", "value1").unwrap();
        store.set("key2", "value2").unwrap();

        let all = store.get_all().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_has_config() {
        let (_tmp, db_path) = setup_test_env();
        let store = ConfigStore::open_at(&db_path).unwrap();

        assert!(!store.has_config().unwrap());

        store.set("key", "value").unwrap();
        assert!(store.has_config().unwrap());
    }

    #[test]
    fn test_load_empty_config() {
        let (_tmp, db_path) = setup_test_env();
        let store = ConfigStore::open_at(&db_path).unwrap();

        let config = store.load_config().unwrap();
        assert!(config.is_none());
    }

    #[test]
    fn test_save_and_load_config() {
        let (_tmp, db_path) = setup_test_env();
        let store = ConfigStore::open_at(&db_path).unwrap();

        let config = Config::default();
        store.save_config(&config).unwrap();

        let partial = store.load_config().unwrap();
        assert!(partial.is_some());

        let partial = partial.unwrap();
        assert_eq!(partial.eval_timeout, Some(config.eval_timeout));
        assert_eq!(partial.human_in_the_loop, Some(config.human_in_the_loop));
    }

    #[test]
    fn test_update_config_value() {
        let (_tmp, db_path) = setup_test_env();
        let store = ConfigStore::open_at(&db_path).unwrap();

        store.set("eval_timeout", "1800").unwrap();
        store.set("eval_timeout", "3600").unwrap();

        let value = store.get("eval_timeout").unwrap();
        assert_eq!(value, Some("3600".to_string()));
    }
}
