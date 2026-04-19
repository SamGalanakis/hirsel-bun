//! Generic runtime settings: a flat key → JSON-value store.
//!
//! Purpose: keep tunable knobs out of the code. Every setting has a typed
//! default owned by the feature that reads it, with an optional value stored
//! in the DB that overrides the default. Tools accept an optional per-call
//! override parameter that beats both.
//!
//! Lookup order inside a tool:
//!   1. Explicit argument, if provided.
//!   2. DB override, if present.
//!   3. Hard default from [`Defaults`].
//!
//! Keys use dotted namespace strings (e.g. `"project_recent_focus.limit"`).

use serde::de::DeserializeOwned;
use serde::Serialize;
use surrealdb::types::SurrealValue;

use crate::backend::db::global_db;

const RUNTIME_SETTING_TABLE: &str = "runtime_setting";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, SurrealValue)]
struct RuntimeSettingRecord {
    key: String,
    value_json: String,
}

/// Typed default values for known settings. Features should read defaults
/// from here instead of hard-coding them inline.
pub struct Defaults;

impl Defaults {
    /// Number of recent focus entries injected into the prompt per turn.
    pub const PROJECT_RECENT_FOCUS_LIMIT: usize = 8;

    /// Max tokens per subgraph chunk when splitting large graphs.
    pub const GRAPH_CHUNK_MAX_TOKENS: usize = 30_000;

    /// Max comments surfaced per node by default.
    pub const GRAPH_COMMENT_DISPLAY_LIMIT: usize = 20;

    /// Comments-per-node threshold that triggers librarian summarisation.
    pub const GRAPH_COMMENT_SUMMARY_THRESHOLD: usize = 50;

    /// How often the summariser sweep runs (seconds).
    pub const GRAPH_COMMENT_SUMMARY_INTERVAL_SEC: u64 = 1_800;

    /// Default timeout when a caller doesn't pass one to `await_thread`.
    pub const THREAD_AWAIT_DEFAULT_TIMEOUT_MS: u64 = 30_000;

    /// Max spawn-depth before the runtime refuses further recursion.
    pub const THREAD_SPAWN_MAX_DEPTH: usize = 5;

    /// Max children a single turn can spawn via `spawn_thread_batch`.
    pub const THREAD_SPAWN_MAX_CHILDREN_PER_TURN: usize = 16;

    /// Auto / on / off for the CoW reflink copy path.
    pub const WORKSPACE_COPY_COW_MODE: &str = "auto";

    /// Prune the thread's workspace copy once a merge succeeds.
    pub const WORKSPACE_COPY_PRUNE_AFTER_MERGE: bool = true;

    /// Merge strategy: "auto" picks git when the workspace is a repo.
    pub const WORKSPACE_MERGE_STRATEGY: &str = "auto";

    /// Days before an orphan workspace copy is eligible for cleanup.
    pub const WORKSPACE_ORPHAN_TTL_DAYS: u64 = 7;

    /// Orphan cleanup behaviour: "prompt" | "auto" | "off".
    pub const WORKSPACE_ORPHAN_CLEANUP: &str = "prompt";

    /// Whether to log shell commands that escape the workspace.
    pub const WORKSPACE_NONWS_INSTRUMENTATION_ENABLED: bool = true;
}

/// Canonical setting keys. Callers reference these to avoid typos.
pub mod keys {
    pub const PROJECT_RECENT_FOCUS_LIMIT: &str = "project_recent_focus.limit";
    pub const GRAPH_CHUNK_MAX_TOKENS: &str = "graph.chunk.max_tokens";
    pub const GRAPH_COMMENT_DISPLAY_LIMIT: &str = "graph.comment.display_limit";
    pub const GRAPH_COMMENT_SUMMARY_THRESHOLD: &str = "graph.comment.summary_threshold";
    pub const GRAPH_COMMENT_SUMMARY_INTERVAL_SEC: &str = "graph.comment.summary_interval_sec";
    pub const THREAD_AWAIT_DEFAULT_TIMEOUT_MS: &str = "thread.await.default_timeout_ms";
    pub const THREAD_SPAWN_MAX_DEPTH: &str = "thread.spawn.max_depth";
    pub const THREAD_SPAWN_MAX_CHILDREN_PER_TURN: &str = "thread.spawn.max_children_per_turn";
    pub const WORKSPACE_COPY_COW_MODE: &str = "workspace.copy.cow_mode";
    pub const WORKSPACE_COPY_PRUNE_AFTER_MERGE: &str = "workspace.copy.prune_after_merge";
    pub const WORKSPACE_MERGE_STRATEGY: &str = "workspace.merge.strategy";
    pub const WORKSPACE_ORPHAN_TTL_DAYS: &str = "workspace.orphan_ttl_days";
    pub const WORKSPACE_ORPHAN_CLEANUP: &str = "workspace.orphan_cleanup";
    pub const WORKSPACE_NONWS_INSTRUMENTATION_ENABLED: &str =
        "workspace.nonws_instrumentation.enabled";
}

pub struct RuntimeSettings;

impl RuntimeSettings {
    /// Look up a setting; return the stored value if present, else the
    /// provided default.
    pub async fn get_or<T>(key: &str, default: T) -> T
    where
        T: DeserializeOwned,
    {
        match Self::load::<T>(key).await {
            Ok(Some(value)) => value,
            _ => default,
        }
    }

    /// Resolve the effective value: per-call override > DB override > default.
    pub async fn resolve<T>(key: &str, override_value: Option<T>, default: T) -> T
    where
        T: DeserializeOwned,
    {
        if let Some(value) = override_value {
            return value;
        }
        Self::get_or(key, default).await
    }

    pub async fn load<T>(key: &str) -> Result<Option<T>, String>
    where
        T: DeserializeOwned,
    {
        let db = global_db().await;
        let record: Option<RuntimeSettingRecord> = db
            .select((RUNTIME_SETTING_TABLE, key))
            .await
            .map_err(|error| format!("failed to load setting {key}: {error}"))?;
        match record {
            None => Ok(None),
            Some(record) => {
                let value = serde_json::from_str::<T>(&record.value_json)
                    .map_err(|error| format!("failed to decode setting {key}: {error}"))?;
                Ok(Some(value))
            }
        }
    }

    pub async fn set<T>(key: &str, value: &T) -> Result<(), String>
    where
        T: Serialize,
    {
        let db = global_db().await;
        let value_json = serde_json::to_string(value)
            .map_err(|error| format!("failed to encode setting {key}: {error}"))?;
        let record = RuntimeSettingRecord {
            key: key.to_string(),
            value_json,
        };
        let _: Option<RuntimeSettingRecord> = db
            .upsert((RUNTIME_SETTING_TABLE, key))
            .content(record)
            .await
            .map_err(|error| format!("failed to save setting {key}: {error}"))?;
        Ok(())
    }

    pub async fn clear(key: &str) -> Result<(), String> {
        let db = global_db().await;
        let _: Option<RuntimeSettingRecord> = db
            .delete((RUNTIME_SETTING_TABLE, key))
            .await
            .map_err(|error| format!("failed to clear setting {key}: {error}"))?;
        Ok(())
    }
}
