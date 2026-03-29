use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use axum::extract::{Form, Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Redirect, Response};

use crate::backend::webui::{
    render_chat_panel, render_focus_document, render_new_project_page, render_project_focus_stage,
    render_project_page, render_project_settings_page, render_threads_panel,
};
use crate::backend::{app, shepherd_runtime};

use super::super::AppState;
use super::support::{
    current_default_sandbox_image, detect_remote_flake, draft_from_confirm_form, draft_from_form,
    ensure_llm_ready, humanize_chat_send_error, load_project_page_state,
    normalize_project_sandbox_image, patch_elements, patch_signals,
    persist_project_and_start_survey, validate_remote_project_source, ChatSendForm,
    ConfirmCreateProjectForm, CreateProjectForm, RemoteProjectValidation, UpdateProjectForm,
};

pub async fn app_home(
    State(state): State<Arc<AppState>>,
) -> Result<Response, (StatusCode, String)> {
    if let Err(response) = ensure_llm_ready(&state, "/app").await {
        return Ok(response);
    }

    let projects = app::list_projects()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if let Some(project) = projects.first() {
        Ok(Redirect::to(&format!("/app/projects/{}", project.id)).into_response())
    } else {
        Ok(Redirect::to("/app/new").into_response())
    }
}

pub async fn new_project_page(
    State(state): State<Arc<AppState>>,
) -> Result<Response, (StatusCode, String)> {
    if let Err(response) = ensure_llm_ready(&state, "/app/new").await {
        return Ok(response);
    }

    let projects = app::list_projects()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let draft = crate::backend::webui::ProjectCreateDraft::blank(current_default_sandbox_image());
    Ok(render_new_project_page(&projects, &draft).into_response())
}

pub async fn setup_project(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateProjectForm>,
) -> Result<Response, (StatusCode, String)> {
    if let Err(response) = ensure_llm_ready(&state, "/app/new").await {
        return Ok(response);
    }

    let projects = app::list_projects()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let parsed = crate::backend::git::parse_github_url(&form.repo_url);
    let requested_branch = form
        .branch
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or(parsed.branch.clone());

    match validate_remote_project_source(&parsed.repo_url, requested_branch.as_deref()) {
        Ok(RemoteProjectValidation::Ready) => {
            let flake_detected =
                detect_remote_flake(&parsed.repo_url, requested_branch.as_deref()).unwrap_or(false);
            let draft = draft_from_form(
                &CreateProjectForm {
                    name: form.name,
                    repo_url: parsed.repo_url,
                    branch: requested_branch,
                    sandbox_image: form.sandbox_image,
                },
                None,
                Some(crate::backend::webui::ProjectCreateReview {
                    needs_remote_branch: false,
                    base_branch: None,
                    flake_detected,
                }),
            );
            Ok(render_new_project_page(&projects, &draft).into_response())
        }
        Ok(RemoteProjectValidation::ConfirmCreateBranch { base_branch }) => {
            let flake_detected =
                detect_remote_flake(&parsed.repo_url, base_branch.as_deref()).unwrap_or(false);
            let draft = draft_from_form(
                &CreateProjectForm {
                    name: form.name,
                    repo_url: parsed.repo_url,
                    branch: Some(requested_branch.unwrap_or_else(|| "main".to_string())),
                    sandbox_image: form.sandbox_image,
                },
                None,
                Some(crate::backend::webui::ProjectCreateReview {
                    needs_remote_branch: true,
                    base_branch,
                    flake_detected,
                }),
            );
            Ok(render_new_project_page(&projects, &draft).into_response())
        }
        Err(error) => {
            let draft = draft_from_form(
                &CreateProjectForm {
                    name: form.name,
                    repo_url: parsed.repo_url,
                    branch: requested_branch,
                    sandbox_image: form.sandbox_image,
                },
                Some(error),
                None,
            );
            Ok(render_new_project_page(&projects, &draft).into_response())
        }
    }
}

pub async fn create_project(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateProjectForm>,
) -> Result<Response, (StatusCode, String)> {
    if let Err(response) = ensure_llm_ready(&state, "/app/new").await {
        return Ok(response);
    }

    let projects = app::list_projects()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let parsed = crate::backend::git::parse_github_url(&form.repo_url);
    let requested_branch = form
        .branch
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or(parsed.branch.clone());

    match validate_remote_project_source(&parsed.repo_url, requested_branch.as_deref()) {
        Ok(RemoteProjectValidation::Ready) => {}
        Ok(RemoteProjectValidation::ConfirmCreateBranch { base_branch }) => {
            let flake_detected =
                detect_remote_flake(&parsed.repo_url, base_branch.as_deref()).unwrap_or(false);
            let draft = draft_from_form(
                &CreateProjectForm {
                    name: form.name,
                    repo_url: parsed.repo_url,
                    branch: Some(requested_branch.unwrap_or_else(|| "main".to_string())),
                    sandbox_image: form.sandbox_image,
                },
                Some("Inspect the repository first so Hirsel can confirm whether this branch must be created.".to_string()),
                Some(crate::backend::webui::ProjectCreateReview {
                    needs_remote_branch: true,
                    base_branch,
                    flake_detected,
                }),
            );
            return Ok(render_new_project_page(&projects, &draft).into_response());
        }
        Err(error) => {
            let draft = draft_from_form(
                &CreateProjectForm {
                    name: form.name,
                    repo_url: parsed.repo_url,
                    branch: requested_branch,
                    sandbox_image: form.sandbox_image,
                },
                Some(error),
                None,
            );
            return Ok(render_new_project_page(&projects, &draft).into_response());
        }
    }

    let sandbox_image = match normalize_project_sandbox_image(form.sandbox_image.as_deref()) {
        Ok(value) => value,
        Err(error) => {
            let flake_detected =
                detect_remote_flake(&parsed.repo_url, requested_branch.as_deref()).unwrap_or(false);
            let draft = draft_from_form(
                &CreateProjectForm {
                    name: form.name,
                    repo_url: parsed.repo_url,
                    branch: requested_branch,
                    sandbox_image: form.sandbox_image,
                },
                Some(error),
                Some(crate::backend::webui::ProjectCreateReview {
                    needs_remote_branch: false,
                    base_branch: None,
                    flake_detected,
                }),
            );
            return Ok(render_new_project_page(&projects, &draft).into_response());
        }
    };

    let project = persist_project_and_start_survey(
        form.name,
        parsed.repo_url,
        requested_branch,
        sandbox_image,
    )
    .await?;

    Ok(Redirect::to(&format!("/app/projects/{}", project.id)).into_response())
}

pub async fn confirm_create_project(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ConfirmCreateProjectForm>,
) -> Result<Response, (StatusCode, String)> {
    if let Err(response) = ensure_llm_ready(&state, "/app/new").await {
        return Ok(response);
    }

    let projects = app::list_projects()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let parsed = crate::backend::git::parse_github_url(&form.repo_url);
    let branch = form.branch.trim().to_string();
    let base_branch = form
        .base_branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let sandbox_image = match normalize_project_sandbox_image(form.sandbox_image.as_deref()) {
        Ok(value) => value,
        Err(error) => {
            let flake_detected =
                detect_remote_flake(&parsed.repo_url, base_branch.as_deref()).unwrap_or(false);
            let draft = draft_from_confirm_form(&form, Some(error), flake_detected);
            return Ok(render_new_project_page(&projects, &draft).into_response());
        }
    };

    if let Err(error) =
        crate::backend::git::create_remote_branch(&parsed.repo_url, &branch, base_branch.as_deref())
    {
        let message = if let Some(base) = base_branch.as_deref() {
            format!(
                "Could not create branch '{}' from '{}': {}",
                branch, base, error
            )
        } else {
            format!(
                "Could not initialize the repository with branch '{}': {}",
                branch, error
            )
        };
        let flake_detected =
            detect_remote_flake(&parsed.repo_url, base_branch.as_deref()).unwrap_or(false);
        let draft = draft_from_confirm_form(&form, Some(message), flake_detected);
        return Ok(render_new_project_page(&projects, &draft).into_response());
    }

    let project =
        persist_project_and_start_survey(form.name, parsed.repo_url, Some(branch), sandbox_image)
            .await?;

    Ok(Redirect::to(&format!("/app/projects/{}", project.id)).into_response())
}

pub async fn project_page(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
) -> Result<Response, (StatusCode, String)> {
    let return_to = format!("/app/projects/{}", project_id);
    if let Err(response) = ensure_llm_ready(&state, &return_to).await {
        return Ok(response);
    }

    let page = load_project_page_state(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(render_project_page(
        &page.projects,
        &page.project,
        &page.surface,
        &page.threads,
        &page.history,
        &page.activity,
    )
    .into_response())
}

pub async fn project_focus_page(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
) -> Result<Response, (StatusCode, String)> {
    let return_to = format!("/app/projects/{}", project_id);
    if let Err(response) = ensure_llm_ready(&state, &return_to).await {
        return Ok(response);
    }

    let surface = app::get_project_surface(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(
        axum::response::Html(render_focus_document(&surface.focus_view.html).into_string())
            .into_response(),
    )
}

pub async fn send_chat_message(
    Path(project_id): Path<i64>,
    Form(form): Form<ChatSendForm>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let stream = stream! {
        match shepherd_runtime::send_project_message(project_id, Some(form.content.clone()), None).await {
            Ok(_) => {
                yield Ok::<Event, Infallible>(patch_signals("{chatDraft: '', chatError: ''}"));
                if let Ok(page) = load_project_page_state(project_id).await {
                    let chat_markup =
                        render_chat_panel(page.project.id, &page.threads, &page.history, &page.activity)
                            .into_string();
                    yield Ok::<Event, Infallible>(patch_elements("#chat-panel", chat_markup));
                }
            }
            Err(error) => {
                let message = humanize_chat_send_error(error);
                let escaped_message = serde_json::to_string(&message)
                    .unwrap_or_else(|_| "\"Failed to send message.\"".to_string());
                let escaped_draft = serde_json::to_string(&form.content)
                    .unwrap_or_else(|_| "\"\"".to_string());
                yield Ok::<Event, Infallible>(patch_signals(format!(
                    "{{chatDraft: {}, chatError: {}}}",
                    escaped_draft, escaped_message
                )));
            }
        }
    };
    Ok(Sse::new(stream))
}

pub async fn project_settings_page(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
) -> Result<Response, (StatusCode, String)> {
    let return_to = format!("/app/projects/{}/settings", project_id);
    if let Err(response) = ensure_llm_ready(&state, &return_to).await {
        return Ok(response);
    }

    let store = crate::backend::project::ProjectStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(render_project_settings_page(&project, &current_default_sandbox_image()).into_response())
}

pub async fn save_project_settings(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
    Form(form): Form<UpdateProjectForm>,
) -> Result<Response, (StatusCode, String)> {
    let return_to = format!("/app/projects/{}/settings", project_id);
    if let Err(response) = ensure_llm_ready(&state, &return_to).await {
        return Ok(response);
    }

    let sandbox_image = normalize_project_sandbox_image(form.sandbox_image.as_deref())
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    app::update_project_settings(project_id, form.name, form.description, sandbox_image)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Redirect::to(&format!("/app/projects/{}/settings", project_id)).into_response())
}

pub async fn delete_project(Path(project_id): Path<i64>) -> Result<Redirect, (StatusCode, String)> {
    let store = crate::backend::project::ProjectStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    store
        .delete_project(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Redirect::to("/app"))
}

pub async fn project_stream(Path(project_id): Path<i64>) -> impl IntoResponse {
    let stream = stream! {
        let mut last_focus = String::new();
        let mut last_chat = String::new();
        let mut last_threads = String::new();

        loop {
            if let Ok(page) = load_project_page_state(project_id).await {
                let focus_markup = render_project_focus_stage(&page.project, &page.surface).into_string();
                let chat_markup =
                    render_chat_panel(page.project.id, &page.threads, &page.history, &page.activity)
                        .into_string();
                let threads_markup = render_threads_panel(&page.project, &page.threads).into_string();

                if focus_markup != last_focus {
                    last_focus = focus_markup.clone();
                    yield Ok::<Event, Infallible>(patch_elements("#focus-panel", focus_markup));
                }
                if chat_markup != last_chat {
                    last_chat = chat_markup.clone();
                    yield Ok::<Event, Infallible>(patch_elements("#chat-panel", chat_markup));
                }
                if threads_markup != last_threads {
                    last_threads = threads_markup.clone();
                    yield Ok::<Event, Infallible>(patch_elements("#threads-panel", threads_markup));
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    };

    Sse::new(stream)
}
