//! Project types and data structures

use serde::{Deserialize, Serialize};

use crate::backend::draft::StartingPoint;

/// Project - one shepherd, one canvas, many threads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub description: Option<String>,

    /// Icon URL (favicon, avatar, etc.) or null for auto-generated initials
    #[serde(default)]
    pub icon: Option<String>,

    /// Source used to materialize the central checkout.
    pub starting_point: StartingPoint,

    /// Optional project-specific container image override.
    #[serde(default)]
    pub sandbox_image: Option<String>,

    // Canvas position (for portfolio view)
    pub x: Option<f64>,
    pub y: Option<f64>,
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
pub struct ProjectSurfaceSnapshot {
    pub focus_view: ProjectFocusView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPreparationStep {
    pub id: String,
    pub label: String,
    pub status: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub progress: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRuntimePreparation {
    pub project_id: i64,
    pub status: String,
    pub headline: String,
    #[serde(default)]
    pub detail: Option<String>,
    pub progress: f64,
    pub steps: Vec<ProjectPreparationStep>,
    pub started_at: String,
    pub updated_at: String,
}

/// Request to create a new project
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub name: String,
    pub starting_point: StartingPoint,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub sandbox_image: Option<String>,
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
    pub sandbox_image: Option<String>,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
}
