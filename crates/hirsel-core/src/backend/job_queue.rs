//! Shared lifecycle helpers for the queued-job tables
//! (`librarian_job`, `kg_chunk_job`). Each table stores a `status` field
//! that moves through `queued → running → completed | failed`, plus an
//! optional `last_error`. The three helpers below cover the transitions
//! that every worker loop needs; row-shape differences and claim logic
//! stay in the caller so deserialisation and fine-grained claim tuning
//! aren't hidden behind a generic trait.

use surrealdb::types::RecordId;

use crate::backend::db::DbClient;

/// Flip a claimed row to `completed` and clear any `last_error`.
pub async fn mark_finished(db: &DbClient, id: &RecordId) -> Result<(), String> {
    db.query("UPDATE $id SET status = 'completed', last_error = NONE")
        .bind(("id", id.clone()))
        .await
        .map_err(|e| format!("mark job completed: {e}"))?;
    Ok(())
}

/// Flip a claimed row to `failed` with the worker's error string.
pub async fn mark_failed(db: &DbClient, id: &RecordId, error: &str) -> Result<(), String> {
    db.query("UPDATE $id SET status = 'failed', last_error = $error")
        .bind(("id", id.clone()))
        .bind(("error", error.to_string()))
        .await
        .map_err(|e| format!("mark job failed: {e}"))?;
    Ok(())
}

/// On worker startup, promote any `running` rows back to `queued`. A
/// `running` row with no matching worker process is always a straggler
/// from a previous process lifetime.
pub async fn requeue_running(db: &DbClient, table: &str) -> Result<(), String> {
    let query = format!("UPDATE {table} SET status = 'queued' WHERE status = 'running'");
    db.query(query)
        .await
        .map_err(|e| format!("requeue stale {table} rows: {e}"))?;
    Ok(())
}
