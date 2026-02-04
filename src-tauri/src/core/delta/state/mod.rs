//! Database operations for draft/live trees and delta submissions
//!
//! This module handles all SQLite operations for the delta dispatch system.
//! It is split into submodules by functional area.

mod deliveries;
mod draft;
mod error;
mod live;
mod orchestration;
mod relations;
mod runs;
mod schema;
mod submissions;
mod versions;

pub use error::{DeltaStateError, DeltaStateResult};
pub use schema::ensure_schema;

use sqlx::SqlitePool;

use crate::core::db::global_pool;
use crate::core::names::slugify;

/// State manager for delta operations
///
/// Manages draft and live trees for a specific project+route combination.
/// Each route has independent trees, runs, versions, and deliveries.
pub struct DeltaState {
    project_id: i64,
    route_id: i64,
}

impl DeltaState {
    /// Create a delta state manager for a specific project and route
    ///
    /// Both project_id and route_id are required - this ensures correct data isolation
    /// between routes.
    pub fn with_route(project_id: i64, route_id: i64) -> Self {
        Self {
            project_id,
            route_id,
        }
    }

    /// Get the project ID
    pub fn project_id(&self) -> i64 {
        self.project_id
    }

    /// Get the route ID
    pub fn route_id(&self) -> i64 {
        self.route_id
    }

    /// Get the global pool with schema initialized
    pub(crate) async fn pool(&self) -> DeltaStateResult<&'static SqlitePool> {
        let pool = global_pool().await;
        ensure_schema(pool).await?;
        Ok(pool)
    }

    /// Generate a unique slug ID (unique within this project)
    pub(crate) async fn generate_slug(
        &self,
        pool: &SqlitePool,
        table: &str,
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
            let exists: bool = sqlx::query_scalar(&format!(
                "SELECT EXISTS(SELECT 1 FROM {} WHERE id = ? AND project_id = ? AND route_id = ?)",
                table
            ))
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
