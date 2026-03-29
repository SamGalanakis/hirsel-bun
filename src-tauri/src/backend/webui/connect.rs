use maud::{html, Markup};

use crate::backend::icons::icon;

use super::shared::app_document;

pub fn render_connect_page(error: Option<&str>, return_to: Option<&str>) -> Markup {
    app_document(
        "Connect",
        "Connect to a Hirsel backend",
        html! {
            main class="connect-page" {
                section class="card connect-card" {
                    header {
                        h2 { "Connect to Hirsel" }
                        p { "Enter the backend API key to open this Hirsel server." }
                    }
                    section {
                        @if let Some(error) = error {
                            div class="alert alert-destructive" {
                                (icon("x"))
                                strong { "Error" }
                                section { p { (error) } }
                            }
                        }
                        form action="/connect/session" method="post" {
                            input type="hidden" name="return_to" value=(return_to.unwrap_or("/app"));
                            div class="field" {
                                label for="api-key" { "API key" }
                                input id="api-key" type="password" name="api_key" autocomplete="current-password" required;
                            }
                            button type="submit" class="btn btn-full" {
                                (icon("key"))
                                "Open Hirsel"
                            }
                        }
                    }
                }
            }
        },
    )
}
