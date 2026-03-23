//! Project types and data structures

use serde::{Deserialize, Serialize};

use crate::core::route::CreateRouteRepoRequest;

/// Project - a lightweight configuration container for route-based work
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub description: Option<String>,

    // Canvas position (for OneBoard portfolio view)
    pub x: Option<f64>,
    pub y: Option<f64>,

    // Active route ID for this project
    pub active_route_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFocusView {
    pub project_id: i64,
    pub html: String,
    pub updated_at: String,
    #[serde(default)]
    pub source: Option<String>,
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
pub struct RouteSummary {
    pub route_id: i64,
    pub name: String,
    pub selected: bool,
    pub status: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSurfaceSnapshot {
    pub focus_view: ProjectFocusView,
    pub routes: Vec<RouteSummary>,
}

/// Request to create a new project
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub name: String,
    pub repos: Vec<CreateRouteRepoRequest>,
    #[serde(default)]
    pub default_repo_index: Option<usize>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
}

/// Request to update a project
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
}
