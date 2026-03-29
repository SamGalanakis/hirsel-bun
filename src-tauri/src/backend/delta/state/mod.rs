//! Database operations for the unified board tree
//!
//! This module handles all SQLite operations for board nodes.
//! It is split into submodules by functional area.

mod deliveries;
mod error;
mod nodes;
mod orchestration;
mod relations;
mod runs;
mod schema;
mod versions;

pub use error::{DeltaStateError, DeltaStateResult};
pub use runs::list_working_route_runtimes;
pub use schema::ensure_schema;

use sqlx::SqlitePool;

use crate::backend::db::global_pool;
use crate::backend::names::slugify;

/// Get a generation counter value (returns 0 if not set)
pub async fn get_generation(key: &str) -> Result<i64, DeltaStateError> {
    let pool = global_pool().await;
    ensure_schema(pool).await?;
    let val: Option<i64> = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(val.unwrap_or(0))
}

/// Atomically increment a generation counter
pub async fn bump_generation(key: &str) -> Result<(), DeltaStateError> {
    let pool = global_pool().await;
    ensure_schema(pool).await?;
    sqlx::query(
        "INSERT INTO meta (key, value) VALUES (?, 1) ON CONFLICT(key) DO UPDATE SET value = value + 1",
    )
    .bind(key)
    .execute(pool)
    .await?;
    Ok(())
}

/// State manager for board operations
///
/// Manages the unified board tree for a specific project+route combination.
/// Each route has independent trees, runs, versions, and deliveries.
pub struct DeltaState {
    project_id: i64,
    route_id: i64,
}

impl DeltaState {
    /// Create a board state manager for a specific project and route
    pub fn with_route(project_id: i64, route_id: i64) -> Self {
        Self {
            project_id,
            route_id,
        }
    }

    /// Get the global pool with schema initialized
    pub(crate) async fn pool(&self) -> DeltaStateResult<&'static SqlitePool> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(pool)
    }

    /// Get the tree generation counter for this project+route
    pub async fn tree_generation(&self) -> DeltaStateResult<i64> {
        get_generation(&format!("trees:{}:{}", self.project_id, self.route_id)).await
    }

    /// Bump the tree generation counter for this project+route
    pub async fn bump_tree_generation(&self) -> DeltaStateResult<()> {
        bump_generation(&format!("trees:{}:{}", self.project_id, self.route_id)).await
    }

    /// Generate a unique slug ID (unique within this project+route)
    pub(crate) async fn generate_slug(
        &self,
        pool: &SqlitePool,
        name: &str,
    ) -> DeltaStateResult<String> {
        let base_slug = slugify(name);
        let slug = if base_slug.is_empty() {
            "node".to_string()
        } else {
            base_slug
        };

        let mut candidate = slug.clone();
        let mut counter = 1;
        loop {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM board_nodes WHERE id = ? AND project_id = ? AND route_id = ?)",
            )
            .bind(&candidate)
            .bind(self.project_id)
            .bind(self.route_id)
            .fetch_one(pool)
            .await?;

            if !exists {
                return Ok(candidate);
            }

            counter += 1;
            candidate = format!("{}-{}", slug, counter);
        }
    }
}
