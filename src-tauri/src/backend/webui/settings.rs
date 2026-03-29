use maud::{html, Markup};

use crate::backend::icons::icon;
use crate::backend::project::Project;

use super::shared::app_document;

pub fn render_settings_page(
    provider: &str,
    openrouter_base_url: Option<&str>,
    openrouter_masked: Option<&str>,
    codex_connected: bool,
    tavily_masked: Option<&str>,
    codex_state: Option<(&str, &str, &str)>,
    setup_required: bool,
    setup_error: Option<&str>,
) -> Markup {
    let page_title = if setup_required {
        "Choose provider"
    } else {
        "Backend settings"
    };
    let page_description = if setup_required {
        "Choose and connect an LLM provider"
    } else {
        "Configure backend providers and services"
    };

    app_document(
        page_title,
        page_description,
        html! {
            main class="shell shell-single" {
                section class="main-panel main-panel-narrow" data-signals:provider-choice=(format!("'{}'", provider)) {
                    div class="panel-header" {
                        div {
                            @if setup_required {
                                p class="eyebrow" { "LLM provider" }
                                h1 { "Choose how Hirsel should think" }
                                p class="muted" {
                                    "Pick a provider, then finish that provider's setup before opening projects."
                                }
                            } @else {
                                p class="eyebrow" { "Backend" }
                                h1 { "Settings" }
                            }
                        }
                        div id="settings-header-action" {
                            @if !setup_required {
                                a href="/app" class="btn btn-ghost" {
                                    (icon("arrow-left"))
                                    "Back"
                                }
                            }
                        }
                    }
                    @if let Some(error) = setup_error {
                        @if !error.trim().is_empty() {
                            div id="settings-setup-alert" class="settings-inline-alert" {
                                p class="text-destructive" { (error) }
                            }
                        } @else {
                            div id="settings-setup-alert" {}
                        }
                    } @else {
                        div id="settings-setup-alert" {}
                    }

                    article class="card" {
                        header {
                            h3 { (icon("cpu")) "LLM Provider" }
                        }
                        section {
                            div class="provider-switch" {
                                button
                                    type="button"
                                    class="provider-option"
                                    data-class:active="$providerChoice === 'codex'"
                                    data-on:click="$providerChoice = 'codex'" {
                                    div class="provider-option-title" { "Codex" }
                                    p class="muted" { "OpenAI account connection for Hirsel." }
                                }
                                button
                                    type="button"
                                    class="provider-option"
                                    data-class:active="$providerChoice === 'openrouter'"
                                    data-on:click="$providerChoice = 'openrouter'" {
                                    div class="provider-option-title" { "OpenRouter" }
                                    p class="muted" { "API key access with optional custom base URL." }
                                }
                            }
                        }
                    }

                    article class="card" id="codex-status" data-show="$providerChoice === 'codex'" {
                        header {
                            h3 { (icon("key")) "Codex" }
                            span data-slot="card-action" {
                                @if codex_connected {
                                    span class="pill status-working" { "Connected" }
                                }
                            }
                        }
                        section {
                            @if codex_connected {
                                p class="muted" { "Codex OAuth is connected. Sessions will use Codex for model inference." }
                                @if provider != "codex" {
                                    form action="/app/settings/llm" method="post" {
                                        input type="hidden" name="provider" value="codex";
                                        button type="submit" class="btn btn-full" {
                                            "Use Codex"
                                        }
                                    }
                                }
                            } @else if let Some((device_auth_id, user_code, verify_url)) = codex_state {
                                div data-init=(format!("@get('/app/settings/codex/stream?device_auth_id={}&user_code={}', {{openWhenHidden: true}})", device_auth_id, user_code)) {}
                                p class="muted" { "Open the verification page, enter the code, and keep this page open." }
                                div class="verification-block" {
                                    div class="field" {
                                        label { "Code" }
                                        div class="verification-value-row" {
                                            span class="code-block" { (user_code) }
                                            button
                                                type="button"
                                                class="btn btn-sm btn-ghost"
                                                data-on:click=(format!("navigator.clipboard.writeText({})", serde_json::to_string(user_code).unwrap_or_else(|_| "\"\"".to_string()))) {
                                                (icon("clipboard"))
                                                "Copy code"
                                            }
                                        }
                                    }
                                    div class="field" {
                                        label { "Verification URL" }
                                        div class="verification-value-row" {
                                            span class="code-block code-block-url" { (verify_url) }
                                            button
                                                type="button"
                                                class="btn btn-sm btn-ghost"
                                                data-on:click=(format!("navigator.clipboard.writeText({})", serde_json::to_string(verify_url).unwrap_or_else(|_| "\"\"".to_string()))) {
                                                (icon("clipboard"))
                                                "Copy URL"
                                            }
                                        }
                                    }
                                }
                                div class="verification-actions" {
                                    button
                                        type="button"
                                        class="btn btn-ghost"
                                        data-on:click=(format!("window.open({}, '_blank', 'noopener,noreferrer')", serde_json::to_string(verify_url).unwrap_or_else(|_| "\"\"".to_string()))) {
                                        (icon("globe"))
                                        "Open Codex verification"
                                    }
                                }
                                p class="eyebrow" { "Waiting for approval…" }
                            } @else {
                                form action="/app/settings/codex/start" method="post" {
                                    button type="submit" class="btn btn-primary btn-full" {
                                        (icon("link"))
                                        "Connect Codex"
                                    }
                                }
                            }
                        }
                    }

                    article class="card" data-show="$providerChoice === 'openrouter'" {
                        header {
                            h3 { (icon("key")) "OpenRouter" }
                            @if provider == "openrouter" {
                                span data-slot="card-action" {
                                    span class="pill status-working" { "Active" }
                                }
                            }
                        }
                        section {
                            form action="/app/settings/openrouter" method="post" {
                                div class="field" {
                                    label for="or-base" { "Base URL" }
                                    input id="or-base" type="text" name="openrouter_base_url" value=(openrouter_base_url.unwrap_or("")) placeholder="https://openrouter.ai/api/v1";
                                }
                                div class="field" {
                                    label for="or-key" { "OpenRouter API key" }
                                    @if let Some(masked) = openrouter_masked {
                                        p class="muted" { "Current: " (masked) }
                                    }
                                    input id="or-key" type="password" name="api_key" placeholder="sk-or-...";
                                }
                                button type="submit" class="btn btn-full" {
                                    (icon("save"))
                                    "Use OpenRouter"
                                }
                            }
                        }
                    }

                    @if !setup_required {
                        article class="card" {
                            header {
                                h3 { (icon("key")) "Services" }
                            }
                            section {
                                form action="/app/settings/tavily" method="post" {
                                    div class="field" {
                                        label for="tav-key" { "Tavily API key" }
                                        @if let Some(masked) = tavily_masked {
                                            p class="muted" { "Current: " (masked) }
                                        }
                                        input id="tav-key" type="password" name="api_key" placeholder="tvly-...";
                                    }
                                    button type="submit" class="btn btn-ghost" {
                                        (icon("save"))
                                        "Save Tavily key"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

pub fn render_project_settings_page(project: &Project, default_sandbox_image: &str) -> Markup {
    let sandbox_image_value = project
        .sandbox_image
        .as_deref()
        .unwrap_or(default_sandbox_image);
    app_document(
        "Project settings",
        "Configure project defaults",
        html! {
            main class="shell shell-single" {
                section class="main-panel main-panel-narrow" {
                    div class="panel-header" {
                        div {
                            p class="eyebrow" { (&project.name) }
                            h1 { "Settings" }
                        }
                        a href=(format!("/app/projects/{}", project.id)) class="btn btn-ghost" {
                            (icon("arrow-left"))
                            "Back"
                        }
                    }

                    article class="card" data-signals:confirm-delete="false" {
                        section {
                            h3 { (icon("folder")) "Project" }
                            form action={ "/app/projects/" (project.id) "/settings/project" } method="post" {
                                div class="field-grid-2" {
                                    div class="field" {
                                        label for="proj-name" { "Name" }
                                        input id="proj-name" type="text" name="name" value=(&project.name);
                                    }
                                    div class="field" {
                                        label for="proj-desc" { "Description" }
                                        input id="proj-desc" type="text" name="description" placeholder="What is this project about?" value=(project.description.as_deref().unwrap_or(""));
                                    }
                                }
                                div class="field" {
                                    label for="proj-image" { "Base image override" }
                                    input
                                        id="proj-image"
                                        type="text"
                                        name="sandbox_image"
                                        value=(sandbox_image_value)
                                        placeholder=(default_sandbox_image);
                                    p class="field-help" {
                                        "Default image: "
                                        code { (default_sandbox_image) }
                                        ". Keep the default unless this project truly needs a different host image."
                                    }
                                }
                                button type="submit" class="btn btn-full" {
                                    (icon("save"))
                                    "Save project"
                                }
                            }
                        }
                        section class="card-danger" {
                            button
                                type="button"
                                class="btn btn-ghost btn-danger"
                                data-show="!$confirmDelete"
                                data-on:click="$confirmDelete = true" {
                                (icon("trash-2"))
                                "Delete project"
                            }
                            form
                                action={ "/app/projects/" (project.id) "/delete" }
                                method="post"
                                data-show="$confirmDelete" {
                                button type="submit" class="btn btn-danger confirmed" {
                                    "Confirm delete"
                                }
                                button type="button" class="btn btn-ghost" data-on:click="$confirmDelete = false" {
                                    "Cancel"
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}
