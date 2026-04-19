//! Implicit read-mark instrumentation.
//!
//! Whenever a thread fetches a node (via `search_context`, direct lookups,
//! etc.) the caller records a read-mark. Other queries can then answer
//! "which threads have seen this node?" and "what has thread X looked at?"
//! without any explicit participation from the LM.

use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use super::db::{global_db, DbClient};

const KG_READ_TABLE: &str = "kg_read";

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct ReadRecord {
    project_id: i64,
    node_kind: String,
    node_id: String,
    thread_id: Option<String>,
    turn: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentRead {
    pub node_kind: String,
    pub node_id: String,
    pub last_read_at: String,
    pub read_count: i64,
}

pub struct ReadStore;

impl ReadStore {
    pub async fn open() -> Result<Self, surrealdb::Error> {
        let _ = global_db().await;
        Ok(Self)
    }

    async fn db(&self) -> &'static DbClient {
        global_db().await
    }

    /// Append a read event. Fire-and-forget — errors are logged but never
    /// propagated because read-tracking is instrumentation, not business.
    pub async fn record(
        &self,
        project_id: i64,
        node_kind: &str,
        node_id: &str,
        thread_id: Option<&str>,
        turn: Option<i64>,
    ) {
        let db = self.db().await;
        let id = uuid::Uuid::new_v4().to_string();
        let record = ReadRecord {
            project_id,
            node_kind: node_kind.to_string(),
            node_id: node_id.to_string(),
            thread_id: thread_id.map(ToOwned::to_owned),
            turn,
        };
        if let Err(error) = db
            .create::<Option<ReadRecord>>((KG_READ_TABLE, id.as_str()))
            .content(record)
            .await
        {
            tracing::debug!(%error, node_kind, node_id, "kg_read record failed");
        }
    }

    /// Record many read events in one pass (e.g. all nodes returned by a
    /// single `search_context` call).
    pub async fn record_batch(
        &self,
        project_id: i64,
        pairs: &[(String, String)],
        thread_id: Option<&str>,
        turn: Option<i64>,
    ) {
        for (kind, id) in pairs {
            self.record(project_id, kind, id, thread_id, turn).await;
        }
    }

    /// Recent nodes touched by a specific thread. Used by the comments
    /// prompt injector to decide which comments are "relevant".
    pub async fn recent_for_thread(
        &self,
        thread_id: &str,
        limit: usize,
    ) -> Result<Vec<RecentRead>, String> {
        let db = self.db().await;
        let mut response = db
            .query(
                "SELECT node_kind, node_id, count() AS read_count, \
                        math::max(read_at) AS last_read_at \
                 FROM kg_read WHERE thread_id = $tid \
                 GROUP BY node_kind, node_id \
                 ORDER BY last_read_at DESC LIMIT $limit",
            )
            .bind(("tid", thread_id.to_string()))
            .bind(("limit", limit as i64))
            .await
            .map_err(|e| format!("recent_for_thread query failed: {e}"))?;
        #[derive(Deserialize, SurrealValue)]
        struct Row {
            node_kind: String,
            node_id: String,
            read_count: i64,
            last_read_at: surrealdb::types::Value,
        }
        let rows: Vec<Row> = response.take(0).unwrap_or_default();
        Ok(rows
            .into_iter()
            .map(|r| RecentRead {
                node_kind: r.node_kind,
                node_id: r.node_id,
                last_read_at: crate::backend::knowledge_graph::surreal_datetime_value_to_string(
                    Some(r.last_read_at),
                ),
                read_count: r.read_count,
            })
            .collect())
    }
}
