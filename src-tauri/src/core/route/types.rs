//! Route types

use serde::{Deserialize, Serialize};

use crate::core::draft::StartingPoint;

/// Linked repository/workspace configuration for a route
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteRepo {
    pub id: i64,
    pub project_id: i64,
    pub route_id: i64,
    pub name: String,
    pub starting_point: StartingPoint,
    pub target_branch: Option<String>,
    pub is_archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Request to create a route repo
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRouteRepoRequest {
    #[serde(default)]
    pub name: Option<String>,
    pub starting_point: StartingPoint,
    #[serde(default)]
    pub target_branch: Option<String>,
}

/// Request to update a route repo
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRouteRepoRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub starting_point: Option<StartingPoint>,
    #[serde(default)]
    pub target_branch: Option<String>,
    #[serde(default)]
    pub is_archived: Option<bool>,
}

/// Route-level execution and delivery settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRouteSettingsRequest {
    pub time_limit_minutes: Option<i64>,
    pub human_in_the_loop: bool,
    pub target_branch: Option<String>,
}

/// A route within a project (parallel exploration branch)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    /// Parent route (for ancestry tracking)
    pub parent_route_id: Option<i64>,
    /// Board version this route was forked from
    pub parent_version_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
    pub repos: Vec<RouteRepo>,
    pub default_repo_id: Option<i64>,
    pub time_limit_minutes: Option<i64>,
    pub human_in_the_loop: bool,
    pub target_branch: Option<String>,
    pub archived_at: Option<String>,
}

/// Route with ancestry information for tree display
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTree {
    pub id: i64,
    pub name: String,
    pub parent_route_id: Option<i64>,
    pub parent_version_id: Option<i64>,
    pub children: Vec<RouteTree>,
    pub created_at: String,
}

impl From<Route> for RouteTree {
    fn from(route: Route) -> Self {
        Self {
            id: route.id,
            name: route.name,
            parent_route_id: route.parent_route_id,
            parent_version_id: route.parent_version_id,
            children: vec![],
            created_at: route.created_at,
        }
    }
}

/// Request to create a new route
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRouteRequest {
    pub name: String,
    /// Fork from this route (defaults to active route)
    pub parent_route_id: Option<i64>,
    /// Fork from this specific version (defaults to latest)
    pub parent_version_id: Option<i64>,
}

/// Seed data for creating a project's main route.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMainRouteRequest {
    pub repos: Vec<CreateRouteRepoRequest>,
    #[serde(default)]
    pub default_repo_index: Option<usize>,
    #[serde(default)]
    pub time_limit_minutes: Option<i64>,
    #[serde(default)]
    pub human_in_the_loop: Option<bool>,
    #[serde(default)]
    pub target_branch: Option<String>,
}
