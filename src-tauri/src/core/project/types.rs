//! Project types and data structures

use serde::{Deserialize, Serialize};

use crate::core::draft::StartingPoint;

/// Project - a lightweight configuration container for runs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub starting_point: StartingPoint,

    // Default configuration (None = use global defaults)
    pub worker_scale: Option<String>,
    pub time_limit_minutes: Option<i64>,
    pub max_iterations: Option<i64>,
    pub human_in_the_loop: bool,
    pub docs_path: String,
    pub persist_docs_changes: bool,
    pub description: Option<String>,

    // Delivery configuration
    pub target_branch: Option<String>, // e.g., "staging", "main" - branch for PR/merge delivery
}

/// Request to create a new project
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub name: String,
    pub starting_point: StartingPoint,
    #[serde(default)]
    pub worker_scale: Option<String>,
    #[serde(default)]
    pub time_limit_minutes: Option<i64>,
    #[serde(default)]
    pub max_iterations: Option<i64>,
    #[serde(default)]
    pub human_in_the_loop: Option<bool>,
    #[serde(default)]
    pub docs_path: Option<String>,
    #[serde(default)]
    pub persist_docs_changes: Option<bool>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub target_branch: Option<String>,
}

/// Request to update a project
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectRequest {
    #[serde(default)]
    pub starting_point: Option<StartingPoint>,
    #[serde(default)]
    pub worker_scale: Option<String>,
    #[serde(default)]
    pub time_limit_minutes: Option<i64>,
    #[serde(default)]
    pub max_iterations: Option<i64>,
    #[serde(default)]
    pub human_in_the_loop: Option<bool>,
    #[serde(default)]
    pub docs_path: Option<String>,
    #[serde(default)]
    pub persist_docs_changes: Option<bool>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub target_branch: Option<String>,
}
