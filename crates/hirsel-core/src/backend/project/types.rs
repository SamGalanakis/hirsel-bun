//! Project types and data structures

use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

/// A workspace attached to a project — a local directory or a remote git repo.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkspaceEntry {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
}

/// Project — persistent context container for conversations, threads, and knowledge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub workspaces: Vec<ProjectWorkspaceEntry>,
    #[serde(default)]
    pub shepherd_cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRetainedContext {
    pub project_id: i64,
    pub markdown: String,
    pub updated_at: String,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSurfaceSnapshot {
    pub canvas: Option<crate::backend::documents::ProjectCanvasDocument>,
}

/// Request to create a new project
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub name: String,
}

/// Request to update a project
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectRequest {
    #[serde(default)]
    pub name: Option<String>,
}
