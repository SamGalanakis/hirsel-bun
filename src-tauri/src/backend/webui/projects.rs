use maud::{html, Markup};

use crate::backend::icons::icon;
use crate::backend::project::{Project, ProjectRuntimePreparation, ProjectSurfaceSnapshot};
use crate::backend::shepherd_runtime::ShepherdScopeActivity;
use crate::backend::ShepherdChatMessage;

use super::conversation::render_chat_panel;
use super::focus::render_project_focus_stage;
use super::shared::app_document;
use super::threads::render_threads_panel;
use super::ThreadPanelState;

#[derive(Debug, Clone)]
pub struct ProjectCreateReview {
    pub needs_remote_branch: bool,
    pub base_branch: Option<String>,
    pub flake_detected: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectCreateDraft {
    pub error: Option<String>,
    pub name: String,
    pub repo_url: String,
    pub branch: String,
    pub default_sandbox_image: String,
    pub sandbox_image: String,
    pub review: Option<ProjectCreateReview>,
}

impl ProjectCreateDraft {
    pub fn blank(default_sandbox_image: String) -> Self {
        Self {
            error: None,
            name: String::new(),
            repo_url: String::new(),
            branch: "main".to_string(),
            sandbox_image: default_sandbox_image.clone(),
            default_sandbox_image,
            review: None,
        }
    }
}

pub fn render_new_project_page(projects: &[Project], draft: &ProjectCreateDraft) -> Markup {
    let has_projects = !projects.is_empty();
    let review = draft.review.as_ref();
    let final_form_action = match review {
        Some(ProjectCreateReview {
            needs_remote_branch: true,
            ..
        }) => "/app/projects/confirm-create",
        _ => "/app/projects",
    };
    let sandbox_image_value = if draft.sandbox_image.trim().is_empty() {
        draft.default_sandbox_image.as_str()
    } else {
        draft.sandbox_image.as_str()
    };
    app_document(
        "New project",
        "Inspect a repository and choose its runtime contract",
        html! {
            main class="welcome-page" {
                div class="welcome-container welcome-container-wide" {
                    svg class="welcome-glyph" viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="0.6" {
                        rect x="4" y="4" width="40" height="40" {}
                        line x1="4" y1="24" x2="44" y2="24" {}
                        line x1="24" y1="4" x2="24" y2="44" {}
                        rect x="14" y="14" width="20" height="20" opacity="0.25" {}
                    }
                    p class="welcome-brand" { "HIRSEL" }

                    div class="new-project-shell" {
                        @if has_projects {
                            aside class="new-project-sidebar" {
                                p class="eyebrow" { "Your projects" }
                                nav class="welcome-project-list" {
                                    @for project in projects {
                                        a href=(format!("/app/projects/{}", project.id)) class="welcome-project-item" {
                                            (icon("folder"))
                                            span { (&project.name) }
                                            (icon("arrow-left"))
                                        }
                                    }
                                }
                            }
                        }

                        section class="welcome-create new-project-stage" {
                            div class="new-project-hero" {
                                @if review.is_some() {
                                    p class="eyebrow" { "Project setup" }
                                    h1 class="welcome-headline" { "Confirm the runtime contract" }
                                    p class="muted" { "Hirsel will treat the repository flake as the project environment and use this container image as the host substrate." }
                                } @else if !has_projects {
                                    h1 class="welcome-headline" { "Inspect a repository before you wire it in" }
                                    p class="muted" { "Hirsel will probe for a flake, show the current default container image, and let you override it before the project exists." }
                                } @else {
                                    p class="eyebrow" { "New project" }
                                    h1 class="welcome-headline" { "Inspect before create" }
                                    p class="muted" { "Keep the source honest, keep the runtime explicit." }
                                }
                            }

                            @if let Some(error) = draft.error.as_deref() {
                                @if !error.trim().is_empty() {
                                    div class="settings-inline-alert project-setup-alert" {
                                        p class="text-destructive" { (error) }
                                    }
                                }
                            }

                            div class="project-setup-grid" {
                                @if let Some(review) = review {
                                    article class="card project-setup-card" {
                                        header class="project-setup-card-head" {
                                            h3 { (icon("folder")) "Source inspection" }
                                            span class=(if review.flake_detected { "pill status-working" } else { "pill status-blocked" }) {
                                                (if review.flake_detected { "flake detected" } else { "no flake yet" })
                                            }
                                        }
                                        section class="project-setup-ledger" {
                                            div class="project-setup-row" {
                                                span class="project-setup-label" { "Name" }
                                                code class="project-setup-value" { (&draft.name) }
                                            }
                                            div class="project-setup-row" {
                                                span class="project-setup-label" { "Repository" }
                                                code class="project-setup-value project-setup-value-break" { (&draft.repo_url) }
                                            }
                                            div class="project-setup-row" {
                                                span class="project-setup-label" { "Branch" }
                                                code class="project-setup-value" { (&draft.branch) }
                                            }
                                        }
                                        @if review.needs_remote_branch {
                                            div class="project-setup-note" {
                                                (icon("git-branch"))
                                                @if let Some(base_branch) = review.base_branch.as_deref().filter(|value| !value.trim().is_empty()) {
                                                    p {
                                                        "The branch "
                                                        code { (&draft.branch) }
                                                        " does not exist yet. Hirsel will create it from "
                                                        code { (base_branch) }
                                                        " before project creation."
                                                    }
                                                } @else {
                                                    p {
                                                        "This repository has no visible branches yet. Hirsel will initialize "
                                                        code { (&draft.branch) }
                                                        " with the first commit."
                                                    }
                                                }
                                            }
                                        } @else if review.flake_detected {
                                            div class="project-setup-note project-setup-note-ok" {
                                                (icon("check"))
                                                p { "A root flake is already present. Shepherd and threads will enter this repo through that environment on their next turn." }
                                            }
                                        } @else {
                                            div class="project-setup-note" {
                                                (icon("sparkles"))
                                                p { "No root flake was found. Shepherd can still bootstrap one from project scope, but coding threads will wait until it exists." }
                                            }
                                        }
                                    }

                                    article class="card project-setup-card project-setup-card-accent" {
                                        header class="project-setup-card-head" {
                                            h3 { (icon("cpu")) "Runtime substrate" }
                                            span class="pill" { "docker host + nix env" }
                                        }
                                        section {
                                            form action=(final_form_action) method="post" class="welcome-form project-setup-form-final" {
                                                input type="hidden" name="name" value=(&draft.name);
                                                input type="hidden" name="repo_url" value=(&draft.repo_url);
                                                input type="hidden" name="branch" value=(&draft.branch);
                                                @if let Some(base_branch) = review.base_branch.as_deref().filter(|value| !value.trim().is_empty()) {
                                                    input type="hidden" name="base_branch" value=(base_branch);
                                                }
                                                div class="project-setup-runtime-callout" {
                                                    p class="eyebrow" { "Default image" }
                                                    code class="project-setup-image-default" { (&draft.default_sandbox_image) }
                                                }
                                                div class="field" {
                                                    label for="sandbox-image" { "Base image override" }
                                                    input
                                                        id="sandbox-image"
                                                        type="text"
                                                        name="sandbox_image"
                                                        value=(sandbox_image_value)
                                                        placeholder=(&draft.default_sandbox_image);
                                                    p class="field-help" {
                                                        "Leave this at the default unless the project truly needs a different host image. The flake remains the main environment contract."
                                                    }
                                                }
                                                button type="submit" class="btn btn-primary btn-full" {
                                                    (icon("plus"))
                                                    "Create project"
                                                }
                                            }
                                            a href="/app/new" class="btn btn-ghost btn-full project-setup-secondary" {
                                                "Inspect a different source"
                                            }
                                        }
                                    }
                                } @else {
                                    article class="card project-setup-card project-setup-card-accent" {
                                        header class="project-setup-card-head" {
                                            h3 { (icon("search")) "Repository probe" }
                                            span class="pill" { "preflight" }
                                        }
                                        section {
                                            form
                                                action="/app/projects/setup"
                                                method="post"
                                                class="welcome-form"
                                                data-signals:repo-url=(format!("{:?}", draft.repo_url))
                                                data-signals:project-name=(format!("{:?}", draft.name))
                                                data-computed:repo-base="$repoUrl.trim().replace(/\\/+$/, '').split('/').pop()?.replace(/\\.git$/, '') || ''"
                                                data-effect="if (!$projectName.trim() && $repoBase) { $projectName = $repoBase }" {
                                                div class="field" {
                                                    label for="proj-name" { "Name" }
                                                    input
                                                        id="proj-name"
                                                        type="text"
                                                        name="name"
                                                        placeholder="my-project"
                                                        value=(&draft.name)
                                                        required
                                                        data-bind:project-name;
                                                }
                                                div class="field" {
                                                    label for="repo-url" { "Repository" }
                                                    input
                                                        id="repo-url"
                                                        type="url"
                                                        name="repo_url"
                                                        placeholder="https://github.com/owner/repo"
                                                        value=(&draft.repo_url)
                                                        required
                                                        data-bind:repo-url;
                                                }
                                                div class="field-grid-2" {
                                                    div class="field" {
                                                        label for="branch" { "Branch" }
                                                        input id="branch" type="text" name="branch" value=(&draft.branch);
                                                    }
                                                    div class="field" {
                                                        label for="setup-sandbox-image" { "Base image" }
                                                        input
                                                            id="setup-sandbox-image"
                                                            type="text"
                                                            name="sandbox_image"
                                                            value=(sandbox_image_value)
                                                            placeholder=(&draft.default_sandbox_image);
                                                    }
                                                }
                                                p class="field-help" {
                                                    "Hirsel will inspect the source for a root flake before it creates the project. The image above is the current backend default and can be overridden per project."
                                                }
                                                button type="submit" class="btn btn-primary btn-full" {
                                                    (icon("search"))
                                                    "Inspect repository"
                                                }
                                            }
                                        }
                                    }

                                    article class="card project-setup-card" {
                                        header class="project-setup-card-head" {
                                            h3 { (icon("cpu")) "Runtime contract" }
                                        }
                                        section class="project-setup-ledger" {
                                            div class="project-setup-row" {
                                                span class="project-setup-label" { "Host image" }
                                                code class="project-setup-value project-setup-value-break" { (&draft.default_sandbox_image) }
                                            }
                                            div class="project-setup-row" {
                                                span class="project-setup-label" { "Repo env" }
                                                span class="project-setup-copy" { "Root flake when present" }
                                            }
                                            div class="project-setup-row" {
                                                span class="project-setup-label" { "Fallback" }
                                                span class="project-setup-copy" { "Shepherd bootstrap only" }
                                            }
                                        }
                                        div class="project-setup-note" {
                                            (icon("sparkles"))
                                            p { "The container image is just the substrate. The repo flake stays the real project environment." }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    div class="welcome-footer" {
                        a href="/app/settings" class="btn btn-ghost" {
                            (icon("settings"))
                            "Backend settings"
                        }
                    }
                }
            }
        },
    )
}

fn preparation_status_class(status: &str) -> &'static str {
    match status {
        "done" => "status-done",
        "working" => "status-working",
        "failed" => "status-failed",
        _ => "status-blocked",
    }
}

fn preparation_status_label(status: &str) -> &'static str {
    match status {
        "done" => "done",
        "working" => "working",
        "failed" => "failed",
        _ => "pending",
    }
}

pub fn render_project_preparation_panel(
    project: &Project,
    preparation: &ProjectRuntimePreparation,
    effective_sandbox_image: &str,
) -> Markup {
    let progress_percent = (preparation.progress * 100.0).round().clamp(0.0, 100.0) as i32;
    let ready = preparation.status == "done";
    let failed = preparation.status == "failed";

    html! {
        article class="project-prep-card" {
            div class="project-prep-topline" {
                p class="eyebrow" { "Runtime preparation" }
                span class=(format!("pill {}", preparation_status_class(&preparation.status))) {
                    (preparation_status_label(&preparation.status))
                }
            }

            div class="project-prep-hero" {
                div {
                    h1 class="project-prep-title" { (&preparation.headline) }
                    @if let Some(detail) = preparation.detail.as_deref().filter(|value| !value.trim().is_empty()) {
                        p class="muted project-prep-copy" { (detail) }
                    }
                }
                div class="project-prep-progress-block" {
                    p class="eyebrow" { "Progress" }
                    div class="project-prep-progress-track" {
                        div class="project-prep-progress-fill" style=(format!("width: {}%;", progress_percent)) {}
                    }
                    p class="project-prep-progress-text" { (format!("{}%", progress_percent)) }
                }
            }

            div class="project-prep-grid" {
                section class="project-prep-main" {
                    div class="project-prep-steps" {
                        @for step in &preparation.steps {
                            article class="project-prep-step" {
                                div class="project-prep-step-head" {
                                    h3 {
                                        @if step.status == "done" {
                                            (icon("check"))
                                        } @else if step.status == "working" {
                                            (icon("refresh-cw"))
                                        } @else if step.status == "failed" {
                                            (icon("x"))
                                        } @else {
                                            (icon("clock"))
                                        }
                                        (&step.label)
                                    }
                                    span class=(format!("pill {}", preparation_status_class(&step.status))) {
                                        (preparation_status_label(&step.status))
                                    }
                                }
                                @if let Some(detail) = step.detail.as_deref().filter(|value| !value.trim().is_empty()) {
                                    p class="muted project-prep-step-copy" { (detail) }
                                }
                            }
                        }
                    }

                    @if failed {
                        form action=(format!("/app/projects/{}/prepare/retry", project.id)) method="post" class="project-prep-actions" {
                            button type="submit" class="btn btn-primary" {
                                (icon("refresh-cw"))
                                "Retry runtime preparation"
                            }
                        }
                    } @else if ready {
                        div class="project-prep-actions" {
                            a href=(format!("/app/projects/{}", project.id)) class="btn btn-primary" {
                                (icon("arrow-left"))
                                "Enter project"
                            }
                        }
                    }
                }

                aside class="project-prep-side" {
                    article class="card project-prep-side-card" {
                        header {
                            h3 { (icon("cpu")) "Runtime contract" }
                        }
                        section class="project-setup-ledger" {
                            div class="project-setup-row" {
                                span class="project-setup-label" { "Project" }
                                code class="project-setup-value" { (&project.name) }
                            }
                            div class="project-setup-row" {
                                span class="project-setup-label" { "Host image" }
                                code class="project-setup-value project-setup-value-break" { (effective_sandbox_image) }
                            }
                            div class="project-setup-row" {
                                span class="project-setup-label" { "Repo env" }
                                span class="project-setup-copy" { "Root flake when present" }
                            }
                            div class="project-setup-row" {
                                span class="project-setup-label" { "Thread rule" }
                                span class="project-setup-copy" { "Threads wait for flake.nix" }
                            }
                        }
                    }

                    article class="card project-prep-side-card project-prep-side-card-note" {
                        header {
                            h3 { (icon("git-branch")) "Flake behavior" }
                        }
                        section {
                            p class="muted" {
                                "If the repository has no root flake yet, shepherd still starts with the bootstrap environment so it can author one. Normal coding threads stay blocked until that file exists in the central checkout."
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn render_project_preparation_page(
    projects: &[Project],
    project: &Project,
    preparation: &ProjectRuntimePreparation,
    effective_sandbox_image: &str,
) -> Markup {
    let stream_url = format!("/app/projects/{}/prepare/stream", project.id);
    let redirect_url = format!("/app/projects/{}", project.id);

    app_document(
        &format!("Preparing {}", project.name),
        "Warm the worker image, central checkout, and shepherd before chat opens",
        html! {
            main
                class="app-shell project-prep-shell"
                data-signals:project-picker-open="false"
                data-signals:prep-ready=(if preparation.status == "done" { "true" } else { "false" })
                data-signals:prep-error="''"
                data-signals:prep-redirecting="false"
                data-signals:prep-redirect=(format!("{:?}", redirect_url))
                data-effect="if ($prepReady && !$prepRedirecting) { $prepRedirecting = true; setTimeout(() => { window.location.href = $prepRedirect }, 450) }" {
                div data-init=(format!("@get('{}', {{openWhenHidden: true}})", stream_url)) {}

                header class="titlebar" {
                    div class="titlebar-left" {
                        a href="/app" class="brandmark" { "HIRSEL" }
                        div class="title-divider" {}
                        div class="project-picker" {
                            button type="button" class="btn btn-sm btn-ghost project-chip" data-on:click="$projectPickerOpen = !$projectPickerOpen" {
                                (icon("folder"))
                                (&project.name)
                                (icon("chevron-down"))
                            }
                            div class="project-dropdown" data-show="$projectPickerOpen" {
                                @for p in projects {
                                    @if p.id == project.id {
                                        span class="project-dropdown-item current" {
                                            (icon("folder"))
                                            (&p.name)
                                            (icon("check"))
                                        }
                                    } @else {
                                        a href=(format!("/app/projects/{}", p.id)) class="project-dropdown-item" {
                                            (icon("folder"))
                                            (&p.name)
                                        }
                                    }
                                }
                                hr {}
                                a href="/app/new" class="project-dropdown-item" {
                                    (icon("plus"))
                                    "New project"
                                }
                            }
                        }
                    }
                    div class="titlebar-right" {
                        a href="/app/settings" class="btn-icon" data-tooltip="Settings" {
                            (icon("settings"))
                        }
                    }
                }

                section class="project-prep-stage" {
                    div class="project-prep-backdrop" {}
                    div id="project-preparation-panel" {
                        (render_project_preparation_panel(project, preparation, effective_sandbox_image))
                    }
                }
            }
        },
    )
}

pub fn render_project_page(
    projects: &[Project],
    project: &Project,
    surface: &ProjectSurfaceSnapshot,
    threads: &[ThreadPanelState],
    history: &[ShepherdChatMessage],
    activity: &ShepherdScopeActivity,
) -> Markup {
    let stream_url = format!("/app/projects/{}/stream", project.id);

    app_document(
        &project.name,
        "Hirsel project workspace",
        html! {
            main class="app-shell" data-signals:project-picker-open="false" {
                div data-init=(format!("@get('{}', {{openWhenHidden: true}})", stream_url)) {}

                header class="titlebar" {
                    div class="titlebar-left" {
                        a href="/app" class="brandmark" { "HIRSEL" }
                        div class="title-divider" {}
                        div class="project-picker" {
                            button type="button" class="btn btn-sm btn-ghost project-chip" data-on:click="$projectPickerOpen = !$projectPickerOpen" {
                                (icon("folder"))
                                (&project.name)
                                (icon("chevron-down"))
                            }
                            div class="project-dropdown" data-show="$projectPickerOpen" {
                                @for p in projects {
                                    @if p.id == project.id {
                                        span class="project-dropdown-item current" {
                                            (icon("folder"))
                                            (&p.name)
                                            (icon("check"))
                                        }
                                    } @else {
                                        a href=(format!("/app/projects/{}", p.id)) class="project-dropdown-item" {
                                            (icon("folder"))
                                            (&p.name)
                                        }
                                    }
                                }
                                hr {}
                                a href=(format!("/app/projects/{}/settings", project.id)) class="project-dropdown-item" {
                                    (icon("settings"))
                                    "Project settings"
                                }
                                a href="/app/new" class="project-dropdown-item" {
                                    (icon("plus"))
                                    "New project"
                                }
                            }
                        }
                    }
                    div class="titlebar-right" {
                        a href="/app/settings" class="btn-icon" data-tooltip="Settings" {
                            (icon("settings"))
                        }
                    }
                }

                section class="workbench" {
                    @let canvas_collapsed = surface.focus_view.source.as_deref() == Some("placeholder");
                    section class=(if canvas_collapsed { "surface-stack canvas-collapsed" } else { "surface-stack" }) {
                        (render_project_focus_stage(project, surface))
                        (render_threads_panel(project, threads))
                    }

                    aside class="chat-rail" {
                        (render_chat_panel(project.id, threads, history, activity))
                    }
                }
            }
        },
    )
}
