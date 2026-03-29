//! Async database utilities for SQLite connections using sqlx.
//!
//! Hirsel now uses a single global database for the project/thread model.

use chrono::Utc;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use super::config::global_db_path;

pub type DbPool = SqlitePool;

/// Global database pool for projects, credentials, chat history, and thread metadata.
static GLOBAL_POOL: OnceLock<SqlitePool> = OnceLock::new();

/// Get the global database pool, initializing if needed.
pub async fn global_pool() -> &'static SqlitePool {
    GLOBAL_POOL.get_or_init(|| {
        let path = global_db_path();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime for db init");
            rt.block_on(async {
                create_pool(&path)
                    .await
                    .expect("Failed to create global database pool")
            })
        })
        .join()
        .expect("Failed to join db init thread")
    })
}

/// Create a new SQLite connection pool with standard settings.
async fn create_pool(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(30))
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);

    SqlitePoolOptions::new()
        .max_connections(10)
        .min_connections(0)
        .acquire_timeout(Duration::from_secs(30))
        .connect_with(options)
        .await
}

/// Generate a UTC timestamp string in ISO 8601 format with microsecond precision.
pub fn utc_now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_create_pool() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await.unwrap();

        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");

        let fk_enabled: i32 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(fk_enabled, 1);

        pool.close().await;
    }

    #[test]
    fn test_utc_now_format() {
        let ts = utc_now();
        assert!(ts.ends_with('Z'));
        assert!(ts.contains('T'));
        assert_eq!(ts.len(), 27);
    }
}
