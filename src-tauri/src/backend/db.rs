//! Async database utilities for embedded SurrealDB connections.
//!
//! Hirsel uses a single global embedded database for shared project state,
//! credentials, chat history, and thread metadata.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::{Db, SurrealKv};
use surrealdb::types::SurrealValue;
use surrealdb::Surreal;
use tokio::sync::{Mutex, OnceCell};

use super::config::global_db_path;

pub type DbClient = Surreal<Db>;

static GLOBAL_DB: OnceCell<DbClient> = OnceCell::const_new();
static COUNTER_LOCK: Mutex<()> = Mutex::const_new(());

const APP_SCHEMA: &str = r#"
DEFINE TABLE IF NOT EXISTS counter SCHEMALESS;
DEFINE TABLE IF NOT EXISTS project SCHEMALESS;
DEFINE TABLE IF NOT EXISTS project_focus_view SCHEMALESS;
DEFINE TABLE IF NOT EXISTS project_retained_context SCHEMALESS;
DEFINE TABLE IF NOT EXISTS project_runtime_preparation SCHEMALESS;
DEFINE TABLE IF NOT EXISTS shepherd_chat_message SCHEMALESS;
DEFINE TABLE IF NOT EXISTS shepherd_live_turn SCHEMALESS;
DEFINE TABLE IF NOT EXISTS shepherd_scope_state SCHEMALESS;
DEFINE TABLE IF NOT EXISTS shepherd_thread SCHEMALESS;
DEFINE TABLE IF NOT EXISTS shepherd_session SCHEMALESS;
DEFINE TABLE IF NOT EXISTS credential SCHEMALESS;
DEFINE TABLE IF NOT EXISTS librarian_event SCHEMALESS;
DEFINE TABLE IF NOT EXISTS kg_node SCHEMALESS;
DEFINE TABLE IF NOT EXISTS kg_edge TYPE RELATION SCHEMALESS;
"#;

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct CounterRecord {
    value: i64,
}

/// Get the global database client, initializing it if needed.
pub async fn global_db() -> &'static DbClient {
    GLOBAL_DB
        .get_or_init(|| async {
            let path = global_db_path();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .expect("Failed to create parent directory for SurrealDB");
            }

            let db = Surreal::new::<SurrealKv>(path.to_string_lossy().into_owned())
                .await
                .expect("Failed to initialize global SurrealDB");

            db.use_ns("hirsel")
                .use_db("app")
                .await
                .expect("Failed to select SurrealDB namespace/database");

            db.query(APP_SCHEMA)
                .await
                .expect("Failed to initialize SurrealDB schema");

            db
        })
        .await
}

/// Allocate the next persistent sequence value for a logical counter.
pub async fn next_sequence(name: &str) -> Result<i64, surrealdb::Error> {
    let _guard = COUNTER_LOCK.lock().await;
    let db = global_db().await;

    let current: Option<CounterRecord> = db.select(("counter", name)).await?;
    let next = current.map(|record| record.value + 1).unwrap_or(1);

    let _: Option<CounterRecord> = db
        .upsert(("counter", name))
        .content(CounterRecord { value: next })
        .await?;

    Ok(next)
}

/// Generate a UTC timestamp string in ISO 8601 format with microsecond precision.
pub fn utc_now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utc_now_format() {
        let ts = utc_now();
        assert!(ts.ends_with('Z'));
        assert!(ts.contains('T'));
        assert_eq!(ts.len(), 27);
    }
}
