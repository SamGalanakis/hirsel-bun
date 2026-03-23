//! Route-scoped worker commands.

use std::path::PathBuf;

use crate::cli::config::get_agent_command;
use crate::core::api_types::{SheepConfig, Worker, WorkerLocation};
use crate::core::credentials::load_forwarded_credentials;
use crate::core::delta::DeltaState;
use crate::core::git::create_worker_clone;
use crate::core::state::{Status, WorkerStatus, WorkerUpdate};
use crate::core::{
    create_orchestrator, create_runner, ensure_route_runtime, get_available_name,
    get_route_runtime_name, CapabilityProfile, RunnerSpawnConfig,
};

use super::ResultExt;

fn worker_to_api(worker: crate::core::state::Worker) -> Worker {
    Worker {
        id: worker.id as u32,
        name: worker.name.clone(),
        pid: worker.pid.map(|pid| pid as u32),
        session_id: worker.session_id,
        status: worker.status.into(),
        work_dir: worker.work_dir,
        waiting_thread: worker.waiting_thread,
        location: WorkerLocation::Local,
        last_heartbeat: worker.last_heartbeat,
        created_at: worker.created_at,
        needs_restart: worker.needs_restart,
        session_started_at: worker.session_started_at,
        hitl_waiting: worker.hitl_waiting,
        is_leader: false,
        context_utilization: None,
        input_tokens: None,
        output_tokens: None,
        turns: None,
        current_task: None,
        sheep_config: SheepConfig::from_name(&worker.name, false),
        capability_profile: worker.capability_profile,
    }
}

pub(crate) async fn resolve_route_runtime_name(
    project_id: i64,
    route_id: i64,
) -> Result<Option<String>, String> {
    get_route_runtime_name(project_id, route_id).await
}

fn staging_dir(runtime_dir: &PathBuf) -> PathBuf {
    runtime_dir.join("work").join("staging")
}

pub(crate) async fn delegate_route_worker(
    project_id: i64,
    route_id: i64,
    item_id: &str,
    capability_profile: CapabilityProfile,
    worker_name: Option<String>,
) -> Result<Worker, String> {
    if !matches!(
        capability_profile,
        CapabilityProfile::CodeWorker | CapabilityProfile::OpsWorker
    ) {
        return Err(format!(
            "Only sandbox worker profiles can be delegated here (got {})",
            capability_profile.as_str()
        ));
    }

    let runtime = ensure_route_runtime(project_id, route_id).await?;
    let delta = DeltaState::with_route(project_id, route_id);
    let used_names = runtime
        .state
        .get_workers()
        .await
        .str_err()?
        .into_iter()
        .map(|worker| worker.name)
        .collect::<Vec<_>>();
    let worker_name = worker_name.unwrap_or_else(|| get_available_name(&used_names));

    let existing_worker = runtime.state.get_worker(&worker_name).await.str_err()?;
    if let Some(ref existing) = existing_worker {
        if existing.pid.is_some() && existing.status == WorkerStatus::Working {
            return Err(format!("Worker '{}' is already active", worker_name));
        }
    }

    delta.claim_node(item_id, &worker_name).await.str_err()?;
    delta
        .assign_work_item(
            item_id,
            Some("worker"),
            Some(&worker_name),
            Some(capability_profile),
        )
        .await
        .str_err()?;

    let worker_dir = if existing_worker.is_some() {
        runtime
            .state
            .get_worker(&worker_name)
            .await
            .str_err()?
            .and_then(|worker| worker.work_dir.map(PathBuf::from))
            .unwrap_or_else(|| staging_dir(&runtime.runtime_dir))
    } else if used_names.is_empty() {
        staging_dir(&runtime.runtime_dir)
    } else {
        let project_path = runtime
            .state
            .get_project_path()
            .await
            .str_err()?
            .ok_or_else(|| "Route runtime has no project path".to_string())?;
        create_worker_clone(
            &runtime.runtime_name,
            &PathBuf::from(project_path),
            &worker_name,
            Some(&staging_dir(&runtime.runtime_dir)),
            &crate::core::config::runtimes_dir(),
        )
        .map_err(|error| error.to_string())?
    };

    if existing_worker.is_none() {
        let (cfg, _) = crate::core::config::Config::load()
            .unwrap_or_else(|_| (crate::core::config::Config::default(), vec![]));
        let execution_kind = cfg.sandbox.execution_kind().to_string();
        runtime
            .state
            .add_worker(
                &worker_name,
                worker_dir.to_str().unwrap_or("."),
                &execution_kind,
                Some(capability_profile),
            )
            .await
            .str_err()?;
    } else {
        runtime
            .state
            .update_worker(
                &worker_name,
                WorkerUpdate {
                    capability_profile: Some(Some(capability_profile)),
                    assigned_task_id: Some(Some(item_id.to_string())),
                    ..Default::default()
                },
            )
            .await
            .str_err()?;
    }

    if runtime.state.status().await.str_err()? != Status::Working {
        runtime.state.set_status(Status::Working).await.str_err()?;
    }

    let (cfg, _) = crate::core::config::Config::load()
        .unwrap_or_else(|_| (crate::core::config::Config::default(), vec![]));
    let runner_config = cfg.sandbox_config();
    let runner = create_runner(&runner_config);
    let result = runner
        .spawn(&RunnerSpawnConfig {
            runtime_name: runtime.runtime_name.clone(),
            worker_name: worker_name.clone(),
            work_dir: worker_dir.clone(),
            runtime_dir: runtime.runtime_dir.clone(),
            agent_command: get_agent_command(),
            is_leader: false,
            leader_name: None,
            teammates: None,
            resume_session_id: None,
            env_vars: None,
            credentials: Some(load_forwarded_credentials().await),
            assigned_task_id: Some(item_id.to_string()),
            is_plan_task: false,
        })
        .await
        .map_err(|error| error.to_string())?;

    runtime
        .state
        .update_worker(
            &worker_name,
            WorkerUpdate {
                pid: result.pid.map(|pid| Some(pid as i64)),
                runner_id: Some(result.handle.runner_id),
                runner_type: Some(result.handle.runner_type),
                status: Some(crate::core::state::WorkerStatus::Working),
                assigned_task_id: Some(Some(item_id.to_string())),
                capability_profile: Some(Some(capability_profile)),
                ..Default::default()
            },
        )
        .await
        .str_err()?;

    let worker = runtime
        .state
        .get_worker(&worker_name)
        .await
        .str_err()?
        .ok_or_else(|| format!("Worker '{}' not found after spawn", worker_name))?;
    Ok(worker_to_api(worker))
}

#[tracing::instrument]
#[tauri::command]
pub async fn get_route_workers(project_id: i64, route_id: i64) -> Result<Vec<Worker>, String> {
    let Some(runtime_name) = resolve_route_runtime_name(project_id, route_id).await? else {
        return Ok(Vec::new());
    };

    let orch = create_orchestrator().str_err()?;
    orch.list_workers(&runtime_name).await.str_err()
}

#[tracing::instrument]
#[tauri::command]
pub async fn open_route_worker_terminal(
    project_id: i64,
    route_id: i64,
    worker_name: String,
) -> Result<(), String> {
    use std::process::Command;

    let runtime_name = resolve_route_runtime_name(project_id, route_id)
        .await?
        .ok_or_else(|| "No route runtime exists yet".to_string())?;
    let session_name = format!("hirsel-{}-{}", runtime_name, worker_name);
    let session_exists = Command::new("tmux")
        .args(["has-session", "-t", &session_name])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if !session_exists {
        return Err(format!(
            "No tmux session '{}' found. Worker '{}' may not be running.",
            session_name, worker_name
        ));
    }

    let attach_cmd = format!("tmux attach-session -t {}", session_name);
    let terminals = [
        ("alacritty", vec!["-e", "sh", "-c", &attach_cmd]),
        ("kitty", vec!["sh", "-c", &attach_cmd]),
        ("wezterm", vec!["start", "--", "sh", "-c", &attach_cmd]),
        ("gnome-terminal", vec!["--", "sh", "-c", &attach_cmd]),
        ("konsole", vec!["-e", "sh", "-c", &attach_cmd]),
        ("xterm", vec!["-e", "sh", "-c", &attach_cmd]),
        ("x-terminal-emulator", vec!["-e", "sh", "-c", &attach_cmd]),
    ];

    for (terminal, args) in terminals {
        if Command::new("which")
            .arg(terminal)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
        {
            if Command::new(terminal).args(args).spawn().is_ok() {
                return Ok(());
            }
        }
    }

    Err(format!(
        "Could not find a terminal emulator. Run manually:\n  tmux attach-session -t {}",
        session_name
    ))
}

#[tracing::instrument]
#[tauri::command]
pub async fn restart_route_worker(
    project_id: i64,
    route_id: i64,
    worker_name: String,
) -> Result<(), String> {
    let runtime_name = resolve_route_runtime_name(project_id, route_id)
        .await?
        .ok_or_else(|| "No route runtime exists yet".to_string())?;
    let orch = create_orchestrator().str_err()?;
    orch.restart_worker(&runtime_name, &worker_name)
        .await
        .str_err()
}
