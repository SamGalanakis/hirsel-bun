//! Async database utilities for SQLite connections using sqlx.
//!
//! This module provides connection pool management for all database access:
//! - Global pool for projects, credentials, delta state
//! - Per-run pools for run-specific state

use chrono::Utc;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::RwLock;

use super::config::global_db_path;

pub type DbPool = SqlitePool;

/// Global database pool (projects, credentials, delta state)
/// Uses std::sync::OnceLock to avoid async deadlock issues with tokio::sync::OnceCell
static GLOBAL_POOL: OnceLock<SqlitePool> = OnceLock::new();

/// Per-run database pools
static RUN_POOLS: tokio::sync::OnceCell<Arc<RwLock<HashMap<String, SqlitePool>>>> =
    tokio::sync::OnceCell::const_new();

/// Get the global database pool, initializing if needed.
///
/// The global database at `~/.hirsel/hirsel.db` stores:
/// - Projects
/// - Credentials
/// - Delta state (draft/live trees)
/// - Gyp chat history
/// - Project messages
pub async fn global_pool() -> &'static SqlitePool {
    // Use get_or_init with blocking initialization to avoid async deadlocks
    GLOBAL_POOL.get_or_init(|| {
        let path = global_db_path();

        // Create a blocking runtime for pool initialization in a separate thread
        // to avoid "cannot start a runtime from within a runtime" panics
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

/// Get a per-run database pool, creating if needed.
///
/// Each run has its own database at `~/.hirsel/runs/{run}/hirsel.db` storing:
/// - Run state
/// - Workers
/// - Messages
/// - Evals
/// - Worker events
pub async fn run_pool(run_name: &str) -> SqlitePool {
    let pools = RUN_POOLS
        .get_or_init(|| async { Arc::new(RwLock::new(HashMap::new())) })
        .await;

    // Fast path: check if pool exists with read lock
    if let Some(pool) = pools.read().await.get(run_name) {
        return pool.clone();
    }

    // Slow path: acquire write lock and re-check (another task may have inserted)
    let mut write = pools.write().await;

    // Re-check under write lock to avoid creating duplicate pools
    if let Some(pool) = write.get(run_name) {
        return pool.clone();
    }

    // Create and insert atomically under the write lock
    let db_path = crate::core::config::run_dir(run_name).join("hirsel.db");
    let pool = create_pool(&db_path)
        .await
        .expect("Failed to create run database pool");

    write.insert(run_name.to_string(), pool.clone());
    pool
}

/// Close a per-run database pool.
///
/// Call this when a run is deleted or no longer needed to free resources.
pub async fn close_run_pool(run_name: &str) {
    if let Some(pools) = RUN_POOLS.get() {
        let mut write = pools.write().await;
        if let Some(pool) = write.remove(run_name) {
            pool.close().await;
        }
    }
}

/// Create a new SQLite connection pool with standard settings.
///
/// Configures:
/// - Busy timeout of 30 seconds for concurrent access
/// - WAL journal mode for better read/write performance
/// - Foreign key constraints enabled
/// - Max 10 connections per pool
async fn create_pool(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    // Ensure directory exists
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
///
/// Format: `YYYY-MM-DDTHH:MM:SS.ffffffZ`
///
/// This is the standard timestamp format used across all database stores.
pub fn utc_now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
}

/// Ensure a database's parent directory exists.
///
/// Call this before opening a database if the path may not exist.
pub fn ensure_db_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
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

        // Verify WAL mode is enabled
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");

        // Verify foreign keys are enabled
        let fk_enabled: i32 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(fk_enabled, 1);

        pool.close().await;
    }

    #[test]
    fn test_ensure_db_dir() {
        let dir = tempdir().unwrap();
        let nested_path = dir.path().join("a").join("b").join("c").join("test.db");

        ensure_db_dir(&nested_path).unwrap();
        assert!(nested_path.parent().unwrap().exists());
    }

    #[test]
    fn test_utc_now_format() {
        let ts = utc_now();
        // Should match pattern like "2024-01-15T10:30:45.123456Z"
        assert!(ts.ends_with('Z'));
        assert!(ts.contains('T'));
        assert_eq!(ts.len(), 27); // Fixed length with microseconds
    }
}
