//! HTTP surface for KG comments.
//! `POST /api/projects/:id/nodes/:kind/:node_id/comments`
//! `GET  /api/projects/:id/nodes/:kind/:node_id/comments`
//! `POST /api/projects/:id/comments/:comment_id/resolve`

use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::backend::kg_comment::{CommentStore, CommentTarget};

#[derive(Deserialize)]
pub struct CreateCommentBody {
    pub body: String,
    #[serde(default)]
    pub target: Option<CommentTarget>,
    /// Optional author override. Defaults to `"user"`.
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub only_unresolved: Option<bool>,
}

pub async fn create_comment(
    Path((project_id, kind, node_id)): Path<(i64, String, String)>,
    Json(body): Json<CreateCommentBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = CommentStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let author = body.author.unwrap_or_else(|| "user".to_string());
    let comment = store
        .add(
            project_id,
            &kind,
            &node_id,
            &body.body,
            &author,
            body.target,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(comment))
}

pub async fn list_comments(
    Path((project_id, kind, node_id)): Path<(i64, String, String)>,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = CommentStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let only_unresolved = query.only_unresolved.unwrap_or(true);
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let comments = store
        .list_for_node(project_id, &kind, &node_id, limit, only_unresolved)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(comments))
}

pub async fn resolve_comment(
    Path((_project_id, comment_id)): Path<(i64, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = CommentStore::open()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let comment = store
        .resolve(&comment_id, "user")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(comment))
}
