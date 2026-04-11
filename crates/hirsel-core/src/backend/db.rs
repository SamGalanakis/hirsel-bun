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

const APP_SCHEMA: &str = include_str!("../../../../db/schema/current.surql");

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct CounterRecord {
    value: i64,
}

async fn apply_schema(db: &DbClient) -> Result<(), String> {
    let mut response = db
        .query(APP_SCHEMA)
        .await
        .map_err(|error| format!("failed to apply SurrealDB schema: {}", error))?;
    let errors = response.take_errors();
    if !errors.is_empty() {
        let msgs: Vec<String> = errors
            .into_iter()
            .map(|(i, e)| format!("statement {i}: {e}"))
            .collect();
        return Err(format!("SurrealDB schema errors:\n{}", msgs.join("\n")));
    }
    Ok(())
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

            apply_schema(&db)
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
