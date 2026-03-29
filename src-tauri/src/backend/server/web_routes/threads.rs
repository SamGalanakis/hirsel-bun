use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use axum::extract::{Form, Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};

use crate::backend::shepherd_runtime;
use crate::backend::webui::{render_thread_detail_main, render_thread_detail_page};

use super::super::AppState;
use super::support::{
    ensure_llm_ready, humanize_chat_send_error, load_thread_page_state, patch_elements,
    patch_signals, ChatSendForm,
};

pub async fn send_thread_message(
    Path((project_id, thread_id)): Path<(i64, String)>,
    Form(form): Form<ChatSendForm>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let stream = stream! {
        match shepherd_runtime::send_thread_message(project_id, &thread_id, Some(form.content.clone()), None).await {
            Ok(_) => {
                yield Ok::<Event, Infallible>(patch_signals("{chatDraft: '', chatError: ''}"));
                if let Ok(page) = load_thread_page_state(project_id, &thread_id, 200).await {
                    let markup = render_thread_detail_main(&page.project, &page.item).into_string();
                    yield Ok::<Event, Infallible>(patch_elements("#thread-detail-main", markup));
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

pub async fn thread_detail_page(
    State(state): State<Arc<AppState>>,
    Path((project_id, thread_id)): Path<(i64, String)>,
) -> Result<Response, (StatusCode, String)> {
    let return_to = format!("/app/projects/{}/threads/{}", project_id, thread_id);
    if let Err(response) = ensure_llm_ready(&state, &return_to).await {
        return Ok(response);
    }

    let page = load_thread_page_state(project_id, &thread_id, 200)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(render_thread_detail_page(&page.project, &page.item).into_response())
}

pub async fn thread_detail_stream(
    Path((project_id, thread_id)): Path<(i64, String)>,
) -> impl IntoResponse {
    let stream = stream! {
        let mut last_html = String::new();
        loop {
            let rendered = match load_thread_page_state(project_id, &thread_id, 200).await {
                Ok(page) => render_thread_detail_main(&page.project, &page.item).into_string(),
                Err(_) => String::new(),
            };

            if !rendered.is_empty() && rendered != last_html {
                last_html = rendered.clone();
                yield Ok::<Event, Infallible>(patch_elements("#thread-detail-main", rendered));
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };
    Sse::new(stream)
}
