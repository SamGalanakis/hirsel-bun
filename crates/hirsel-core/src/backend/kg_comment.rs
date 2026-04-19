//! Additive, targetable comments on knowledge-graph nodes.
//!
//! Semantics:
//! - Every `add` inserts a new row. Comments never stomp each other.
//! - `target` is optional — `None` means "the whole node"; otherwise
//!   `{property?, line_start?, line_end?}` narrows the anchor.
//! - Only the author or a privileged caller (project owner) can
//!   `resolve` a comment; other threads leave their own opinion instead.

use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use super::db::{global_db, utc_now, DbClient};

const KG_COMMENT_TABLE: &str = "kg_comment";

/// Persisted row shape.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct CommentRecord {
    comment_id: String,
    project_id: i64,
    node_kind: String,
    node_id: String,
    target_json: Option<String>,
    body: String,
    author: String,
    posted_at: String,
    resolved_at: Option<String>,
}

/// Optional structured anchor within a node.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommentTarget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_end: Option<i64>,
}

impl CommentTarget {
    pub fn is_empty(&self) -> bool {
        self.property.is_none() && self.line_start.is_none() && self.line_end.is_none()
    }
}

/// Public shape surfaced by tools and API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub project_id: i64,
    pub node_kind: String,
    pub node_id: String,
    pub target: Option<CommentTarget>,
    pub body: String,
    pub author: String,
    pub posted_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CommentError {
    #[error("Database error: {0}")]
    Database(#[from] surrealdb::Error),
    #[error("Comment not found: {0}")]
    NotFound(String),
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Forbidden: {0}")]
    Forbidden(String),
}

pub type CommentResult<T> = Result<T, CommentError>;

pub struct CommentStore;

impl CommentStore {
    pub async fn open() -> CommentResult<Self> {
        let _ = global_db().await;
        Ok(Self)
    }

    async fn db(&self) -> &'static DbClient {
        global_db().await
    }

    pub async fn add(
        &self,
        project_id: i64,
        node_kind: &str,
        node_id: &str,
        body: &str,
        author: &str,
        target: Option<CommentTarget>,
    ) -> CommentResult<Comment> {
        let db = self.db().await;
        let id = uuid::Uuid::new_v4().to_string();
        let now = utc_now();
        let target_json = match target.as_ref() {
            Some(t) if !t.is_empty() => Some(serde_json::to_string(t)?),
            _ => None,
        };
        let record = CommentRecord {
            comment_id: id.clone(),
            project_id,
            node_kind: node_kind.to_string(),
            node_id: node_id.to_string(),
            target_json,
            body: body.to_string(),
            author: author.to_string(),
            posted_at: now,
            resolved_at: None,
        };
        let _: Option<CommentRecord> = db
            .create((KG_COMMENT_TABLE, id.as_str()))
            .content(record.clone())
            .await?;

        // T4: if unresolved comments on this node now meet the pileup
        // threshold, enqueue a librarian verify job. Best-effort; never
        // blocks the insert.
        let project_id = record.project_id;
        let kind_owned = record.node_kind.clone();
        let node_id_owned = record.node_id.clone();
        tokio::spawn(async move {
            maybe_enqueue_comment_pileup_verify(project_id, &kind_owned, &node_id_owned).await;
        });

        record.into_comment()
    }

    /// List recent comments on a node. `only_unresolved` filters out rows
    /// with `resolved_at` set. Ordered by `posted_at DESC`, limit applied.
    pub async fn list_for_node(
        &self,
        project_id: i64,
        node_kind: &str,
        node_id: &str,
        limit: usize,
        only_unresolved: bool,
    ) -> CommentResult<Vec<Comment>> {
        let db = self.db().await;
        let mut response = if only_unresolved {
            db.query(
                "SELECT * FROM kg_comment \
                 WHERE project_id = $pid AND node_kind = $kind AND node_id = $nid \
                   AND resolved_at = NONE \
                 ORDER BY posted_at DESC LIMIT $limit",
            )
            .bind(("pid", project_id))
            .bind(("kind", node_kind.to_string()))
            .bind(("nid", node_id.to_string()))
            .bind(("limit", limit as i64))
            .await?
        } else {
            db.query(
                "SELECT * FROM kg_comment \
                 WHERE project_id = $pid AND node_kind = $kind AND node_id = $nid \
                 ORDER BY posted_at DESC LIMIT $limit",
            )
            .bind(("pid", project_id))
            .bind(("kind", node_kind.to_string()))
            .bind(("nid", node_id.to_string()))
            .bind(("limit", limit as i64))
            .await?
        };
        let records: Vec<CommentRecord> = response.take(0).unwrap_or_default();
        records.into_iter().map(|r| r.into_comment()).collect()
    }

    /// Fetch comments touching any of `(kind, node_id)` pairs — used to
    /// inject relevant comments into a thread's prompt.
    pub async fn list_for_any(
        &self,
        project_id: i64,
        pairs: &[(String, String)],
        limit_per_node: usize,
    ) -> CommentResult<Vec<Comment>> {
        if pairs.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for (kind, id) in pairs {
            let comments = self
                .list_for_node(project_id, kind, id, limit_per_node, true)
                .await?;
            out.extend(comments);
        }
        Ok(out)
    }

    pub async fn get(&self, comment_id: &str) -> CommentResult<Comment> {
        let db = self.db().await;
        let record: Option<CommentRecord> = db.select((KG_COMMENT_TABLE, comment_id)).await?;
        record
            .ok_or_else(|| CommentError::NotFound(comment_id.to_string()))?
            .into_comment()
    }

    /// Mark resolved. Caller must supply the resolver's identity; the
    /// comment's own author can always resolve, plus `"user"` for
    /// project-owner overrides.
    pub async fn resolve(&self, comment_id: &str, resolver: &str) -> CommentResult<Comment> {
        let db = self.db().await;
        let mut record: CommentRecord = db
            .select((KG_COMMENT_TABLE, comment_id))
            .await?
            .ok_or_else(|| CommentError::NotFound(comment_id.to_string()))?;
        if record.author != resolver && resolver != "user" {
            return Err(CommentError::Forbidden(format!(
                "{resolver} cannot resolve comment authored by {}",
                record.author
            )));
        }
        record.resolved_at = Some(utc_now());
        let _: Option<CommentRecord> = db
            .upsert((KG_COMMENT_TABLE, comment_id))
            .content(record.clone())
            .await?;
        record.into_comment()
    }

    /// Summarise a node's comment thread by writing a single summary row
    /// and marking the originals as resolved. Used by the librarian.
    pub async fn summarise_and_resolve(
        &self,
        project_id: i64,
        node_kind: &str,
        node_id: &str,
        summary_body: &str,
        summary_author: &str,
        originals: &[String],
    ) -> CommentResult<Comment> {
        let db = self.db().await;
        let now = utc_now();
        for id in originals {
            let mut record: CommentRecord = match db.select((KG_COMMENT_TABLE, id.as_str())).await?
            {
                Some(r) => r,
                None => continue,
            };
            if record.resolved_at.is_some() {
                continue;
            }
            record.resolved_at = Some(now.clone());
            let _: Option<CommentRecord> = db
                .upsert((KG_COMMENT_TABLE, id.as_str()))
                .content(record)
                .await?;
        }
        self.add(
            project_id,
            node_kind,
            node_id,
            summary_body,
            summary_author,
            None,
        )
        .await
    }

    /// Count unresolved comments per node. Returns `(kind, id, count)`
    /// tuples; used by the summarisation sweep to find hot nodes.
    pub async fn unresolved_counts_by_node(
        &self,
    ) -> CommentResult<Vec<(i64, String, String, usize)>> {
        let db = self.db().await;
        let mut response = db
            .query(
                "SELECT project_id, node_kind, node_id, count() AS total \
                 FROM kg_comment WHERE resolved_at = NONE \
                 GROUP BY project_id, node_kind, node_id",
            )
            .await?;
        #[derive(Deserialize, SurrealValue)]
        struct Row {
            project_id: i64,
            node_kind: String,
            node_id: String,
            total: i64,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|r| (r.project_id, r.node_kind, r.node_id, r.total as usize))
            .collect())
    }
}

impl CommentRecord {
    fn into_comment(self) -> CommentResult<Comment> {
        let target = match self.target_json.as_deref() {
            Some(json) if !json.trim().is_empty() => {
                Some(serde_json::from_str::<CommentTarget>(json)?)
            }
            _ => None,
        };
        Ok(Comment {
            id: self.comment_id,
            project_id: self.project_id,
            node_kind: self.node_kind,
            node_id: self.node_id,
            target,
            body: self.body,
            author: self.author,
            posted_at: self.posted_at,
            resolved_at: self.resolved_at,
        })
    }
}

/// Pileup threshold for T4 (comment-driven verify). Matches Phase 4G's
/// worked example — 3 unresolved on a node is "people keep flagging it."
const COMMENT_PILEUP_THRESHOLD: i64 = 3;

async fn maybe_enqueue_comment_pileup_verify(project_id: i64, node_kind: &str, node_id: &str) {
    let db = global_db().await;
    let Ok(mut response) = db
        .query(
            "SELECT count() AS c FROM kg_comment \
             WHERE project_id = $pid AND node_kind = $kind AND node_id = $nid \
               AND resolved_at = NONE GROUP ALL;",
        )
        .bind(("pid", project_id))
        .bind(("kind", node_kind.to_string()))
        .bind(("nid", node_id.to_string()))
        .await
    else {
        return;
    };
    #[derive(serde::Deserialize, SurrealValue)]
    struct CountRow {
        #[serde(default)]
        c: i64,
    }
    let rows: Vec<CountRow> = response.take(0).unwrap_or_default();
    let count = rows.into_iter().next().map(|r| r.c).unwrap_or(0);
    if count < COMMENT_PILEUP_THRESHOLD {
        return;
    }
    let _ = crate::backend::librarian::enqueue_verify_node(
        project_id,
        node_kind,
        node_id,
        &format!("comment_pileup:{count}"),
    )
    .await;
}
