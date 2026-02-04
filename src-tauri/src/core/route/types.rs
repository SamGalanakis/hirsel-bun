//! Route types

use serde::{Deserialize, Serialize};

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
