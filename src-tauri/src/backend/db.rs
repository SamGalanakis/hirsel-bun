//! Async database utilities for embedded SurrealDB connections.
//!
//! Hirsel uses a single global embedded database for shared project state,
//! credentials, chat history, and thread metadata.

use std::collections::HashMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::{Db, SurrealKv};
use surrealdb::opt::auth::Record;
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
DEFINE FIELD IF NOT EXISTS starting_point.starting_point_type ON TABLE project TYPE string;
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

-- Librarian agent identity table (record users for project-scoped access)
DEFINE TABLE IF NOT EXISTS agent SCHEMALESS;

-- Access method for librarian record users
DEFINE ACCESS IF NOT EXISTS librarian ON DATABASE TYPE RECORD
  SIGNUP (CREATE agent SET name = $name, project_id = $project_id)
  SIGNIN (SELECT * FROM agent WHERE name = $name AND project_id = $project_id)
  DURATION FOR TOKEN 24h, FOR SESSION 24h;

-- Knowledge graph node table with librarian permissions
DEFINE TABLE IF NOT EXISTS kg_node SCHEMALESS
  PERMISSIONS
    FOR select WHERE project_id = $auth.project_id
    FOR create WHERE project_id = $auth.project_id
    FOR update WHERE project_id = $auth.project_id
    FOR delete WHERE project_id = $auth.project_id AND NOT (kind = 'document' AND node_id = 'canvas');

-- Knowledge graph edge table with librarian permissions
DEFINE TABLE IF NOT EXISTS kg_edge TYPE RELATION SCHEMALESS
  PERMISSIONS
    FOR select WHERE project_id = $auth.project_id
    FOR create WHERE project_id = $auth.project_id
    FOR update WHERE project_id = $auth.project_id
    FOR delete WHERE project_id = $auth.project_id;

DEFINE TABLE IF NOT EXISTS kg_doc_edge_queue SCHEMALESS;

DEFINE EVENT IF NOT EXISTS doc_content_changed ON TABLE kg_node
    WHEN $after.kind = 'document'
      AND ($event = "CREATE" OR $before.content != $after.content)
    THEN (
        UPSERT type::record('kg_doc_edge_queue', [$after.project_id, $after.node_id])
            SET project_id = $after.project_id,
                node_id = $after.node_id,
                queued_at = time::now()
    );
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

/// Cached librarian DB connections keyed by project_id.
static LIBRARIAN_CONNECTIONS: OnceCell<Mutex<HashMap<i64, DbClient>>> = OnceCell::const_new();

/// Parameters for the librarian record-user signin/signup.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct LibrarianAuthParams {
    name: String,
    project_id: i64,
}

/// Get a DB client authenticated as the librarian record-user for a project.
///
/// The client is scoped via SurrealDB table permissions so it can only access
/// `kg_node` and `kg_edge` rows belonging to the given project. Connections
/// are cached per project_id for the lifetime of the process.
pub async fn librarian_db(project_id: i64) -> Result<DbClient, String> {
    let cache = LIBRARIAN_CONNECTIONS
        .get_or_init(|| async { Mutex::new(HashMap::new()) })
        .await;

    let mut map = cache.lock().await;
    if let Some(client) = map.get(&project_id) {
        return Ok(client.clone());
    }

    // Clone the global DB to get a new session sharing the same engine
    let client = global_db().await.clone();

    let credentials = Record {
        namespace: "hirsel".to_string(),
        database: "app".to_string(),
        access: "librarian".to_string(),
        params: LibrarianAuthParams {
            name: format!("librarian-{project_id}"),
            project_id,
        },
    };

    // Try signin first; if the agent record doesn't exist yet, signup
    let token = match client.signin(credentials).await {
        Ok(token) => token,
        Err(_) => {
            let signup_credentials = Record {
                namespace: "hirsel".to_string(),
                database: "app".to_string(),
                access: "librarian".to_string(),
                params: LibrarianAuthParams {
                    name: format!("librarian-{project_id}"),
                    project_id,
                },
            };
            client
                .signup(signup_credentials)
                .await
                .map_err(|e| format!("librarian signup failed: {e}"))?
        }
    };

    // Authenticate the session with the obtained token
    client
        .authenticate(token)
        .await
        .map_err(|e| format!("librarian authenticate failed: {e}"))?;

    map.insert(project_id, client.clone());
    Ok(client)
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
