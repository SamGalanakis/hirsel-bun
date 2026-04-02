use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use super::types::{ProjectPreparationStep, ProjectRuntimePreparation};
use super::ProjectStore;
use crate::backend::config::Config;
use crate::backend::credentials::require_tavily_api_key;
use crate::backend::db::utc_now;
use crate::backend::sandbox::ensure_sandbox_image_available_with_progress;
use crate::backend::shepherd_runtime::{
    get_shepherd_activity, launch_project_survey_thread, prepare_shepherd_session,
    send_shepherd_message, stop_scope_activity, ShepherdScope,
};
use crate::backend::workspace::ensure_project_workspace;

const STEP_TAVILY: &str = "tavily";
const STEP_WORKSPACE: &str = "workspace";
const STEP_FLAKE: &str = "flake";
const STEP_IMAGE: &str = "image";
const STEP_SHEPHERD: &str = "shepherd";
const STEP_ENV: &str = "environment";

static ACTIVE_PREPARATIONS: OnceLock<Mutex<HashSet<i64>>> = OnceLock::new();

fn active_preparations() -> &'static Mutex<HashSet<i64>> {
    ACTIVE_PREPARATIONS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn mark_active(project_id: i64) -> bool {
    if let Ok(mut active) = active_preparations().lock() {
        return active.insert(project_id);
    }
    false
}

fn clear_active(project_id: i64) {
    if let Ok(mut active) = active_preparations().lock() {
        active.remove(&project_id);
    }
}

fn compute_progress(steps: &[ProjectPreparationStep]) -> f64 {
    if steps.is_empty() {
        return 0.0;
    }

    let complete = steps
        .iter()
        .map(|step| match step.status.as_str() {
            "done" => 1.0,
            "working" | "failed" => step.progress.unwrap_or(0.12).clamp(0.0, 1.0),
            _ => 0.0,
        })
        .sum::<f64>();

    (complete / steps.len() as f64).clamp(0.0, 1.0)
}

fn step_mut<'a>(
    state: &'a mut ProjectRuntimePreparation,
    id: &str,
) -> Option<&'a mut ProjectPreparationStep> {
    state.steps.iter_mut().find(|step| step.id == id)
}

fn effective_image_label(project_image: Option<&str>, default_image: &str) -> String {
    project_image
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_image)
        .to_string()
}

fn build_state(project_id: i64, image: &str) -> ProjectRuntimePreparation {
    let now = utc_now();
    let steps = vec![
        ProjectPreparationStep {
            id: STEP_TAVILY.to_string(),
            label: "Resolve required services".to_string(),
            status: "working".to_string(),
            detail: Some("Checking the required Tavily search key.".to_string()),
            progress: Some(0.06),
        },
        ProjectPreparationStep {
            id: STEP_WORKSPACE.to_string(),
            label: "Clone central checkout".to_string(),
            status: "pending".to_string(),
            detail: Some("Cloning the repository into the shared project workspace.".to_string()),
            progress: Some(0.0),
        },
        ProjectPreparationStep {
            id: STEP_FLAKE.to_string(),
            label: "Resolve repo flake".to_string(),
            status: "pending".to_string(),
            detail: Some(
                "Checking whether the central checkout already has a repo flake.".to_string(),
            ),
            progress: Some(0.0),
        },
        ProjectPreparationStep {
            id: STEP_IMAGE.to_string(),
            label: "Prepare worker substrate".to_string(),
            status: "pending".to_string(),
            detail: Some(format!("Preparing container substrate `{}`.", image)),
            progress: Some(0.0),
        },
        ProjectPreparationStep {
            id: STEP_SHEPHERD.to_string(),
            label: "Wake shepherd".to_string(),
            status: "pending".to_string(),
            detail: Some("Starting the project shepherd so onboarding can continue.".to_string()),
            progress: Some(0.0),
        },
        ProjectPreparationStep {
            id: STEP_ENV.to_string(),
            label: "Enter repo environment".to_string(),
            status: "pending".to_string(),
            detail: Some("Waiting to enter the repository's Nix environment.".to_string()),
            progress: Some(0.0),
        },
    ];

    ProjectRuntimePreparation {
        project_id,
        status: "working".to_string(),
        headline: "Warming the runtime".to_string(),
        detail: Some(
            "Building the substrate, opening the central checkout, and waking shepherd."
                .to_string(),
        ),
        progress: compute_progress(&steps),
        steps,
        started_at: now.clone(),
        updated_at: now,
    }
}

fn advance_step(
    state: &mut ProjectRuntimePreparation,
    id: &str,
    status: &str,
    detail: impl Into<Option<String>>,
    progress: Option<f64>,
) {
    if let Some(step) = step_mut(state, id) {
        step.status = status.to_string();
        step.detail = detail.into();
        step.progress = match status {
            "done" => Some(1.0),
            "pending" => Some(0.0),
            _ => progress.or(step.progress),
        };
    }
    state.progress = compute_progress(&state.steps);
    state.updated_at = utc_now();
}

async fn save_state(
    state: &ProjectRuntimePreparation,
) -> Result<ProjectRuntimePreparation, String> {
    let store = ProjectStore::open()
        .await
        .map_err(|error| format!("failed to open project store: {}", error))?;
    store
        .save_project_runtime_preparation(state)
        .await
        .map_err(|error| error.to_string())
}

fn spawn_survey_if_possible(project_id: i64, has_flake: bool) {
    if !has_flake {
        return;
    }
    tokio::spawn(async move {
        if let Err(error) = launch_project_survey_thread(project_id).await {
            tracing::warn!(project_id, %error, "project survey thread launch failed after runtime preparation");
        }
    });
}

async fn ensure_project_flake(project_id: i64, central_dir: &Path) -> Result<bool, String> {
    if central_dir.join("flake.nix").is_file() {
        return Ok(false);
    }

    send_shepherd_message(
        project_id,
        Some(
            "The central checkout has no `flake.nix`. Create a valid root `flake.nix` for this \
             repository now. Infer the project stack from the repo contents. Keep it minimal but \
             sufficient for normal development in Hirsel. When you are done, stop."
                .to_string(),
        ),
        None,
    )
    .await?;

    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    let mut saw_active_turn = false;
    loop {
        let activity = get_shepherd_activity(project_id).await?;
        let flake_exists = central_dir.join("flake.nix").is_file();
        if activity.has_active_turn {
            saw_active_turn = true;
        } else if flake_exists {
            return Ok(true);
        } else if saw_active_turn {
            return Err(
                "Shepherd finished startup bootstrap without creating `flake.nix` in the central checkout."
                    .to_string(),
            );
        }

        if std::time::Instant::now() >= deadline {
            return Err(
                "Timed out waiting for shepherd to create `flake.nix` in the central checkout."
                    .to_string(),
            );
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn shepherd_scope(project_id: i64) -> ShepherdScope {
    ShepherdScope::Shepherd {
        project_id,
        workspace_path: None,
        focus: None,
    }
}

async fn run_preparation(project_id: i64) -> Result<(), String> {
    let store = ProjectStore::open()
        .await
        .map_err(|error| format!("failed to open project store: {}", error))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|error| error.to_string())?;
    let (config, _) =
        Config::load().map_err(|error| format!("failed to load config: {}", error))?;
    let image = effective_image_label(project.sandbox_image.as_deref(), &config.sandbox.image);

    let mut state = build_state(project_id, &image);
    save_state(&state).await?;

    let result = async {
        let tavily = require_tavily_api_key().await?;
        advance_step(
            &mut state,
            STEP_TAVILY,
            "done",
            Some(match tavily.source {
                crate::backend::credentials::CredentialSource::Env => {
                    "Required Tavily key loaded from the environment.".to_string()
                }
                crate::backend::credentials::CredentialSource::Store => {
                    "Required Tavily key loaded from saved settings.".to_string()
                }
            }),
            Some(1.0),
        );
        advance_step(
            &mut state,
            STEP_WORKSPACE,
            "working",
            Some("Opening the project central checkout.".to_string()),
            Some(0.18),
        );
        save_state(&state).await?;

        let workspace = ensure_project_workspace(project_id).await?;
        advance_step(
            &mut state,
            STEP_WORKSPACE,
            "done",
            Some(format!(
                "Central checkout ready at `{}`.",
                workspace.central_dir.display()
            )),
            Some(1.0),
        );
        let had_flake = workspace.central_dir.join("flake.nix").is_file();
        advance_step(
            &mut state,
            STEP_FLAKE,
            if had_flake { "done" } else { "pending" },
            Some(if had_flake {
                "Repo flake detected in the central checkout.".to_string()
            } else {
                "No repo flake found yet. Shepherd will author one during setup.".to_string()
            }),
            Some(if had_flake { 1.0 } else { 0.0 }),
        );
        advance_step(
            &mut state,
            STEP_IMAGE,
            "working",
            Some(format!("Preparing container substrate `{}`.", image)),
            Some(0.06),
        );
        save_state(&state).await?;

        ensure_sandbox_image_available_with_progress(&image, |update| {
            advance_step(
                &mut state,
                STEP_IMAGE,
                "working",
                Some(update.detail),
                Some(update.progress),
            );
            let snapshot = state.clone();
            async move { save_state(&snapshot).await.map(|_| ()) }
        })
        .await?;
        advance_step(
            &mut state,
            STEP_IMAGE,
            "done",
            Some(format!("Worker substrate `{}` is ready.", image)),
            Some(1.0),
        );
        let _ = stop_scope_activity(shepherd_scope(project_id)).await;
        advance_step(
            &mut state,
            STEP_SHEPHERD,
            "working",
            Some(if had_flake {
                "Starting shepherd inside the repo flake.".to_string()
            } else {
                "Starting shepherd in the bootstrap environment.".to_string()
            }),
            Some(0.18),
        );
        save_state(&state).await?;

        prepare_shepherd_session(project_id).await?;
        advance_step(
            &mut state,
            STEP_SHEPHERD,
            "done",
            Some(if had_flake {
                "Shepherd is warm and waiting inside the repo flake.".to_string()
            } else {
                "Bootstrap shepherd is warm and ready to author the repo flake.".to_string()
            }),
            Some(1.0),
        );
        if had_flake {
            advance_step(
                &mut state,
                STEP_ENV,
                "done",
                Some("Shepherd entered the repo environment immediately.".to_string()),
                Some(1.0),
            );
        } else {
            advance_step(
                &mut state,
                STEP_FLAKE,
                "working",
                Some("Asking shepherd to create `flake.nix` in the central checkout.".to_string()),
                Some(0.62),
            );
            save_state(&state).await?;

            ensure_project_flake(project_id, &workspace.central_dir).await?;
            advance_step(
                &mut state,
                STEP_FLAKE,
                "done",
                Some("Shepherd created `flake.nix` in the central checkout.".to_string()),
                Some(1.0),
            );
            advance_step(
                &mut state,
                STEP_ENV,
                "working",
                Some("Restarting shepherd inside the repo flake.".to_string()),
                Some(0.34),
            );
            save_state(&state).await?;

            stop_scope_activity(shepherd_scope(project_id)).await?;
            prepare_shepherd_session(project_id).await?;
            advance_step(
                &mut state,
                STEP_ENV,
                "done",
                Some("Shepherd restarted inside the repo environment.".to_string()),
                Some(1.0),
            );
        }
        state.status = "done".to_string();
        state.headline = "Runtime ready".to_string();
        state.detail = Some(
            "Conversation is live. Hirsel has a project flake and can start coding threads immediately."
                .to_string(),
        );
        state.progress = 1.0;
        state.updated_at = utc_now();
        save_state(&state).await?;

        spawn_survey_if_possible(project_id, true);
        Ok::<(), String>(())
    }
    .await;

    if let Err(error) = result {
        if step_mut(&mut state, STEP_SHEPHERD).is_some_and(|step| step.status == "working") {
            advance_step(
                &mut state,
                STEP_SHEPHERD,
                "failed",
                Some(error.clone()),
                None,
            );
        } else if step_mut(&mut state, STEP_IMAGE).is_some_and(|step| step.status == "working") {
            advance_step(&mut state, STEP_IMAGE, "failed", Some(error.clone()), None);
        } else if step_mut(&mut state, STEP_TAVILY).is_some_and(|step| step.status == "working") {
            advance_step(&mut state, STEP_TAVILY, "failed", Some(error.clone()), None);
        } else if step_mut(&mut state, STEP_WORKSPACE).is_some_and(|step| step.status == "working")
        {
            advance_step(
                &mut state,
                STEP_WORKSPACE,
                "failed",
                Some(error.clone()),
                None,
            );
        } else if step_mut(&mut state, STEP_ENV).is_some_and(|step| step.status == "working") {
            advance_step(&mut state, STEP_ENV, "failed", Some(error.clone()), None);
        } else if step_mut(&mut state, STEP_FLAKE).is_some_and(|step| step.status == "working") {
            advance_step(&mut state, STEP_FLAKE, "failed", Some(error.clone()), None);
        } else {
            advance_step(&mut state, STEP_IMAGE, "failed", Some(error.clone()), None);
        }
        state.status = "failed".to_string();
        state.headline = "Runtime preparation failed".to_string();
        state.detail = Some(error.clone());
        state.updated_at = utc_now();
        let _ = save_state(&state).await;
        return Err(error);
    }

    Ok(())
}

pub async fn get_project_runtime_preparation(
    project_id: i64,
) -> Result<Option<ProjectRuntimePreparation>, String> {
    let store = ProjectStore::open()
        .await
        .map_err(|error| format!("failed to open project store: {}", error))?;
    store
        .get_project_runtime_preparation(project_id)
        .await
        .map_err(|error| error.to_string())
}

pub async fn ensure_project_runtime_preparation_started(
    project_id: i64,
) -> Result<ProjectRuntimePreparation, String> {
    let store = ProjectStore::open()
        .await
        .map_err(|error| format!("failed to open project store: {}", error))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|error| error.to_string())?;
    let (config, _) =
        Config::load().map_err(|error| format!("failed to load config: {}", error))?;
    let image = effective_image_label(project.sandbox_image.as_deref(), &config.sandbox.image);

    if let Some(state) = store
        .get_project_runtime_preparation(project_id)
        .await
        .map_err(|error| error.to_string())?
    {
        if state.status == "done" {
            return Ok(state);
        }
        if active_preparations()
            .lock()
            .map_err(|_| "failed to lock project preparation state".to_string())?
            .contains(&project_id)
        {
            return Ok(state);
        }
    }

    let state = build_state(project_id, &image);
    store
        .save_project_runtime_preparation(&state)
        .await
        .map_err(|error| error.to_string())?;

    if mark_active(project_id) {
        tokio::spawn(async move {
            let result = run_preparation(project_id).await;
            clear_active(project_id);
            if let Err(error) = result {
                tracing::warn!(project_id, %error, "project runtime preparation failed");
            }
        });
    }

    Ok(state)
}

pub async fn retry_project_runtime_preparation(
    project_id: i64,
) -> Result<ProjectRuntimePreparation, String> {
    if active_preparations()
        .lock()
        .map_err(|_| "failed to lock project preparation state".to_string())?
        .contains(&project_id)
    {
        return get_project_runtime_preparation(project_id)
            .await?
            .ok_or_else(|| "project preparation is already running".to_string());
    }

    let store = ProjectStore::open()
        .await
        .map_err(|error| format!("failed to open project store: {}", error))?;
    let _ = store
        .get_project(project_id)
        .await
        .map_err(|error| error.to_string())?;

    let (config, _) =
        Config::load().map_err(|error| format!("failed to load config: {}", error))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|error| error.to_string())?;
    let image = effective_image_label(project.sandbox_image.as_deref(), &config.sandbox.image);
    let state = build_state(project_id, &image);
    store
        .save_project_runtime_preparation(&state)
        .await
        .map_err(|error| error.to_string())?;

    if mark_active(project_id) {
        tokio::spawn(async move {
            let result = run_preparation(project_id).await;
            clear_active(project_id);
            if let Err(error) = result {
                tracing::warn!(project_id, %error, "project runtime preparation failed after retry");
            }
        });
    }

    Ok(state)
}

pub async fn project_runtime_is_ready(project_id: i64) -> Result<bool, String> {
    Ok(get_project_runtime_preparation(project_id)
        .await?
        .is_some_and(|state| state.status == "done"))
}
