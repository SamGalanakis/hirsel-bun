use std::path::PathBuf;

use crate::core::config;
use crate::core::db::close_runtime_pool;
use crate::core::db::utc_now;
use crate::core::delta::DeltaState;
use crate::core::draft::create_workspace_provider;
use crate::core::ops::{setup_run_workspace, RunSetupConfig};
use crate::core::project::ProjectStore;
use crate::core::route::{Route, RouteStore};
use crate::core::state::{SQLiteState, Status};
use crate::core::Files;

pub struct RouteRuntimeHandle {
    pub runtime_name: String,
    pub runtime_dir: PathBuf,
    pub state: SQLiteState,
    pub route: Route,
}

pub fn runtime_name_for_route(project_id: i64, route_id: i64) -> String {
    format!("project-{}-route-{}", project_id, route_id)
}

pub async fn get_route_runtime_name(
    project_id: i64,
    route_id: i64,
) -> Result<Option<String>, String> {
    let delta = DeltaState::with_route(project_id, route_id);
    Ok(delta
        .get_route_runtime()
        .await
        .map_err(|error| error.to_string())?
        .map(|runtime| runtime.runtime_name))
}

pub async fn ensure_route_runtime(
    project_id: i64,
    route_id: i64,
) -> Result<RouteRuntimeHandle, String> {
    let delta = DeltaState::with_route(project_id, route_id);
    let (runtime_name, created_new) = if let Some(runtime) = delta
        .get_route_runtime()
        .await
        .map_err(|error| error.to_string())?
    {
        (runtime.runtime_name, false)
    } else {
        let runtime_name = runtime_name_for_route(project_id, route_id);
        delta
            .create_route_runtime(&runtime_name)
            .await
            .map_err(|error| error.to_string())?;
        (runtime_name, true)
    };

    let runtime_dir = config::runtime_dir(&runtime_name);
    if created_new && runtime_dir.exists() {
        tracing::warn!(
            runtime_name = %runtime_name,
            runtime_dir = %runtime_dir.display(),
            "fresh route runtime collided with existing runtime directory; removing stale runtime dir"
        );
        close_runtime_pool(&runtime_name).await;
        std::fs::remove_dir_all(&runtime_dir).map_err(|error| {
            format!(
                "failed to remove stale runtime dir '{}': {}",
                runtime_dir.display(),
                error
            )
        })?;
    }

    let route_store = RouteStore::new(project_id)
        .await
        .map_err(|error| error.to_string())?;
    let route = route_store
        .get_route(route_id)
        .await
        .map_err(|error| error.to_string())?;

    let db_path = runtime_dir.join("hirsel.db");
    if db_path.exists() {
        let state = SQLiteState::new(&runtime_name)
            .await
            .map_err(|error| error.to_string())?;
        return Ok(RouteRuntimeHandle {
            runtime_name,
            runtime_dir,
            state,
            route,
        });
    }

    std::fs::create_dir_all(&runtime_dir).map_err(|error| error.to_string())?;
    Files::new(&runtime_dir)
        .init_dirs()
        .map_err(|error| error.to_string())?;

    let starting_point = route_store
        .get_default_repo_starting_point(route_id)
        .await
        .map_err(|error| error.to_string())?;
    let workspace = create_workspace_provider();
    let workspace_info = workspace
        .init(&runtime_name, &starting_point)
        .await
        .map_err(|error| error.to_string())?;

    let project_path = workspace_info.path.clone();
    let state = SQLiteState::new(&runtime_name)
        .await
        .map_err(|error| error.to_string())?;
    state
        .init_state(Some(project_path.to_str().unwrap_or(".")))
        .await
        .map_err(|error| error.to_string())?;

    let project_store = ProjectStore::open()
        .await
        .map_err(|error| error.to_string())?;
    let project = project_store
        .get_project(project_id)
        .await
        .map_err(|error| error.to_string())?;

    state
        .set_project_id(project.id)
        .await
        .map_err(|error| error.to_string())?;
    state
        .set_project_name(&project.name)
        .await
        .map_err(|error| error.to_string())?;
    state
        .set_route_id(route_id)
        .await
        .map_err(|error| error.to_string())?;
    state
        .set_status(Status::Working)
        .await
        .map_err(|error| error.to_string())?;
    let started_at = utc_now();
    state
        .set_started_at(Some(&started_at))
        .await
        .map_err(|error| error.to_string())?;
    state
        .set_human_in_the_loop(route.human_in_the_loop)
        .await
        .map_err(|error| error.to_string())?;

    if let Some(limit) = route.time_limit_minutes {
        state
            .set_time_limit_minutes(Some(limit))
            .await
            .map_err(|error| error.to_string())?;
    }
    if let Some(branch) = workspace_info.default_branch.as_deref() {
        state
            .set_branch(Some(branch))
            .await
            .map_err(|error| error.to_string())?;
    }

    setup_run_workspace(&RunSetupConfig {
        runtime_name: runtime_name.clone(),
        project_path,
        runtime_dir: runtime_dir.clone(),
        worker_names: Vec::new(),
        is_multi_worker: false,
        leader_name: None,
    })
    .map_err(|error| error.to_string())?;

    Ok(RouteRuntimeHandle {
        runtime_name,
        runtime_dir,
        state,
        route,
    })
}
