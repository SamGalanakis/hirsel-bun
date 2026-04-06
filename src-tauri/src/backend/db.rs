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
DEFINE TABLE IF NOT EXISTS counter SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS value ON TABLE counter TYPE int;

DEFINE TABLE IF NOT EXISTS app_setting SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS provider ON TABLE app_setting TYPE string;
DEFINE FIELD IF NOT EXISTS openrouter_base_url ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS shepherd_model ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS shepherd_model_variant ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS librarian_model ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS librarian_model_variant ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS thread_model ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS thread_model_variant ON TABLE app_setting TYPE option<string>;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE app_setting TYPE string;

DEFINE TABLE IF NOT EXISTS project SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE project TYPE int;
DEFINE FIELD IF NOT EXISTS name ON TABLE project TYPE string;
DEFINE FIELD IF NOT EXISTS name_lower ON TABLE project TYPE string;
DEFINE FIELD IF NOT EXISTS workspace_key ON TABLE project TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE project TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE project TYPE string;
DEFINE FIELD IF NOT EXISTS description ON TABLE project TYPE option<string>;
DEFINE FIELD IF NOT EXISTS icon ON TABLE project TYPE option<string>;
DEFINE FIELD IF NOT EXISTS starting_point ON TABLE project TYPE object;
DEFINE FIELD IF NOT EXISTS starting_point.type ON TABLE project TYPE string;
DEFINE FIELD IF NOT EXISTS starting_point.path ON TABLE project TYPE option<string>;
DEFINE FIELD IF NOT EXISTS starting_point.url ON TABLE project TYPE option<string>;
DEFINE FIELD IF NOT EXISTS starting_point.branch ON TABLE project TYPE option<string>;
DEFINE FIELD IF NOT EXISTS sandbox_image ON TABLE project TYPE option<string>;
DEFINE FIELD IF NOT EXISTS x ON TABLE project TYPE option<float>;
DEFINE FIELD IF NOT EXISTS y ON TABLE project TYPE option<float>;
DEFINE INDEX IF NOT EXISTS project_name_lower_idx ON TABLE project FIELDS name_lower UNIQUE;

DEFINE TABLE IF NOT EXISTS project_retained_context SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE project_retained_context TYPE int;
DEFINE FIELD IF NOT EXISTS markdown ON TABLE project_retained_context TYPE string;
DEFINE FIELD IF NOT EXISTS source ON TABLE project_retained_context TYPE option<string>;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE project_retained_context TYPE string;

DEFINE TABLE IF NOT EXISTS project_runtime_preparation SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE project_runtime_preparation TYPE int;
DEFINE FIELD IF NOT EXISTS status ON TABLE project_runtime_preparation TYPE string;
DEFINE FIELD IF NOT EXISTS headline ON TABLE project_runtime_preparation TYPE string;
DEFINE FIELD IF NOT EXISTS detail ON TABLE project_runtime_preparation TYPE option<string>;
DEFINE FIELD IF NOT EXISTS progress ON TABLE project_runtime_preparation TYPE float;
DEFINE FIELD IF NOT EXISTS steps ON TABLE project_runtime_preparation TYPE array<object>;
DEFINE FIELD IF NOT EXISTS steps[*].id ON TABLE project_runtime_preparation TYPE string;
DEFINE FIELD IF NOT EXISTS steps[*].label ON TABLE project_runtime_preparation TYPE string;
DEFINE FIELD IF NOT EXISTS steps[*].status ON TABLE project_runtime_preparation TYPE string;
DEFINE FIELD IF NOT EXISTS steps[*].detail ON TABLE project_runtime_preparation TYPE option<string>;
DEFINE FIELD IF NOT EXISTS steps[*].progress ON TABLE project_runtime_preparation TYPE option<float>;
DEFINE FIELD IF NOT EXISTS current_step_id ON TABLE project_runtime_preparation TYPE option<string>;
DEFINE FIELD IF NOT EXISTS started_at ON TABLE project_runtime_preparation TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE project_runtime_preparation TYPE string;

DEFINE TABLE IF NOT EXISTS shepherd_chat_message SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS message_id ON TABLE shepherd_chat_message TYPE int;
DEFINE FIELD IF NOT EXISTS lookup_key ON TABLE shepherd_chat_message TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE shepherd_chat_message TYPE option<int>;
DEFINE FIELD IF NOT EXISTS scope_key ON TABLE shepherd_chat_message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS role ON TABLE shepherd_chat_message TYPE string;
DEFINE FIELD IF NOT EXISTS timestamp ON TABLE shepherd_chat_message TYPE string;
DEFINE FIELD IF NOT EXISTS chunks_json ON TABLE shepherd_chat_message TYPE string;
DEFINE INDEX IF NOT EXISTS shepherd_chat_lookup_idx ON TABLE shepherd_chat_message FIELDS lookup_key;

DEFINE TABLE IF NOT EXISTS shepherd_live_turn SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS lookup_key ON TABLE shepherd_live_turn TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE shepherd_live_turn TYPE option<int>;
DEFINE FIELD IF NOT EXISTS scope_key ON TABLE shepherd_live_turn TYPE string;
DEFINE FIELD IF NOT EXISTS role ON TABLE shepherd_live_turn TYPE string;
DEFINE FIELD IF NOT EXISTS chunks_json ON TABLE shepherd_live_turn TYPE string;
DEFINE FIELD IF NOT EXISTS status ON TABLE shepherd_live_turn TYPE string;
DEFINE FIELD IF NOT EXISTS error ON TABLE shepherd_live_turn TYPE option<string>;
DEFINE FIELD IF NOT EXISTS started_at ON TABLE shepherd_live_turn TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE shepherd_live_turn TYPE string;

DEFINE TABLE IF NOT EXISTS shepherd_scope_state SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS lookup_key ON TABLE shepherd_scope_state TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE shepherd_scope_state TYPE int;
DEFINE FIELD IF NOT EXISTS scope_key ON TABLE shepherd_scope_state TYPE string;
DEFINE FIELD IF NOT EXISTS state_json ON TABLE shepherd_scope_state TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE shepherd_scope_state TYPE string;

DEFINE TABLE IF NOT EXISTS shepherd_thread SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS thread_id ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE shepherd_thread TYPE int;
DEFINE FIELD IF NOT EXISTS title ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS title_lower ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS objective ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS summary ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS status ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS workspace_path ON TABLE shepherd_thread TYPE option<string>;
DEFINE FIELD IF NOT EXISTS checkout_name ON TABLE shepherd_thread TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS last_activity_at ON TABLE shepherd_thread TYPE string;
DEFINE FIELD IF NOT EXISTS archived_at ON TABLE shepherd_thread TYPE option<string>;
DEFINE INDEX IF NOT EXISTS shepherd_thread_project_idx ON TABLE shepherd_thread FIELDS project_id;
DEFINE INDEX IF NOT EXISTS shepherd_thread_title_idx ON TABLE shepherd_thread FIELDS project_id, title_lower;

DEFINE TABLE IF NOT EXISTS shepherd_session SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE shepherd_session TYPE option<int>;
DEFINE FIELD IF NOT EXISTS scope_key ON TABLE shepherd_session TYPE string;
DEFINE FIELD IF NOT EXISTS scope_json ON TABLE shepherd_session TYPE string;
DEFINE FIELD IF NOT EXISTS workspace_path ON TABLE shepherd_session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS env_fingerprint ON TABLE shepherd_session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS runtime_fingerprint ON TABLE shepherd_session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS status ON TABLE shepherd_session TYPE string;
DEFINE FIELD IF NOT EXISTS container_name ON TABLE shepherd_session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS socket_path ON TABLE shepherd_session TYPE string;
DEFINE FIELD IF NOT EXISTS last_error ON TABLE shepherd_session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE shepherd_session TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE shepherd_session TYPE string;
DEFINE FIELD IF NOT EXISTS last_seen_at ON TABLE shepherd_session TYPE option<string>;

DEFINE TABLE IF NOT EXISTS credential SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS key_type ON TABLE credential TYPE string;
DEFINE FIELD IF NOT EXISTS encrypted_value_b64 ON TABLE credential TYPE string;
DEFINE FIELD IF NOT EXISTS nonce_b64 ON TABLE credential TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE credential TYPE string;

DEFINE TABLE IF NOT EXISTS librarian_event SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE librarian_event TYPE int;
DEFINE FIELD IF NOT EXISTS kind ON TABLE librarian_event TYPE string;
DEFINE FIELD IF NOT EXISTS summary ON TABLE librarian_event TYPE string;
DEFINE FIELD IF NOT EXISTS files ON TABLE librarian_event TYPE array<any>;
DEFINE FIELD IF NOT EXISTS timestamp ON TABLE librarian_event TYPE string;
DEFINE FIELD IF NOT EXISTS processed ON TABLE librarian_event TYPE bool;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE librarian_event TYPE datetime;

DEFINE TABLE IF NOT EXISTS kg_node SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE kg_node TYPE int;
DEFINE FIELD IF NOT EXISTS kind ON TABLE kg_node TYPE string;
DEFINE FIELD IF NOT EXISTS node_id ON TABLE kg_node TYPE string;
DEFINE FIELD IF NOT EXISTS label ON TABLE kg_node TYPE string;
DEFINE FIELD IF NOT EXISTS summary ON TABLE kg_node TYPE option<string>;
DEFINE FIELD IF NOT EXISTS description ON TABLE kg_node TYPE option<string>;
DEFINE FIELD IF NOT EXISTS detail ON TABLE kg_node TYPE option<string>;
DEFINE FIELD IF NOT EXISTS notes ON TABLE kg_node TYPE option<string>;
DEFINE FIELD IF NOT EXISTS rationale ON TABLE kg_node TYPE option<string>;
DEFINE FIELD IF NOT EXISTS markdown ON TABLE kg_node TYPE option<string>;
DEFINE FIELD IF NOT EXISTS body_html ON TABLE kg_node TYPE option<string>;
DEFINE FIELD IF NOT EXISTS confidence ON TABLE kg_node TYPE option<string>;
DEFINE FIELD IF NOT EXISTS source ON TABLE kg_node TYPE option<string>;
DEFINE FIELD IF NOT EXISTS metadata ON TABLE kg_node TYPE option<object> FLEXIBLE;
DEFINE FIELD IF NOT EXISTS updated_at ON TABLE kg_node TYPE string;

DEFINE TABLE IF NOT EXISTS kg_edge TYPE RELATION SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS project_id ON TABLE kg_edge TYPE int;
DEFINE FIELD IF NOT EXISTS relation ON TABLE kg_edge TYPE string;
DEFINE FIELD IF NOT EXISTS metadata ON TABLE kg_edge TYPE option<object> FLEXIBLE;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE kg_edge TYPE datetime;
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
