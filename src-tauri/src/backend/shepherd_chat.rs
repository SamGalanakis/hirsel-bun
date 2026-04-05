//! Shepherd chat, live-turn, and scope-state storage.

use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use super::db::{global_db, next_sequence, utc_now, DbClient};
use crate::backend::live_updates::{self, LiveUpdateKind};

const SHEPHERD_CHAT_MESSAGE_TABLE: &str = "shepherd_chat_message";
const SHEPHERD_LIVE_TURN_TABLE: &str = "shepherd_live_turn";
const SHEPHERD_SCOPE_STATE_TABLE: &str = "shepherd_scope_state";

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct ShepherdChatMessageRecord {
    message_id: i64,
    lookup_key: String,
    project_id: Option<i64>,
    scope_key: Option<String>,
    role: String,
    timestamp: String,
    chunks_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct ShepherdLiveTurnRecord {
    lookup_key: String,
    project_id: Option<i64>,
    scope_key: String,
    role: String,
    chunks_json: String,
    status: String,
    error: Option<String>,
    started_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct ShepherdScopeStateRecord {
    lookup_key: String,
    project_id: i64,
    scope_key: String,
    state_json: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdChatMessage {
    pub id: i64,
    pub project_id: Option<i64>,
    pub scope_key: Option<String>,
    pub role: String,
    pub timestamp: String,
    pub chunks_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdLiveTurn {
    pub project_id: Option<i64>,
    pub scope_key: String,
    pub role: String,
    pub chunks_json: String,
    pub status: String,
    pub error: Option<String>,
    pub started_at: String,
    pub updated_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ShepherdChatError {
    #[error("Database error: {0}")]
    Database(#[from] surrealdb::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type ShepherdChatResult<T> = Result<T, ShepherdChatError>;

pub struct ShepherdChatStore;

impl ShepherdChatStore {
    pub fn shepherd_scope_key(project_id: i64) -> String {
        format!("__shepherd__:{project_id}")
    }

    pub fn librarian_scope_key(project_id: i64) -> String {
        format!("__librarian__:{project_id}")
    }

    pub fn thread_scope_key(thread_id: &str) -> String {
        format!("__thread__:{thread_id}")
    }

    pub async fn open() -> ShepherdChatResult<Self> {
        let _ = global_db().await;
        Ok(Self)
    }

    async fn db(&self) -> &'static DbClient {
        global_db().await
    }

    pub async fn save_message(
        &self,
        scope_key: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> ShepherdChatResult<i64> {
        self.save_message_with_project(None, scope_key, role, chunks_json)
            .await
    }

    pub async fn save_message_with_project(
        &self,
        project_id: Option<i64>,
        scope_key: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> ShepherdChatResult<i64> {
        let db = self.db().await;
        let id = next_sequence("shepherd_chat_message").await?;
        let timestamp = utc_now();
        let record = ShepherdChatMessageRecord {
            message_id: id,
            lookup_key: chat_lookup_key(project_id, scope_key),
            project_id,
            scope_key: scope_key.map(ToOwned::to_owned),
            role: role.to_string(),
            timestamp,
            chunks_json: chunks_json.to_string(),
        };

        let _: Option<ShepherdChatMessageRecord> = db
            .create((SHEPHERD_CHAT_MESSAGE_TABLE, id))
            .content(record.clone())
            .await?;
        publish_history_event(project_id, scope_key);

        Ok(id)
    }

    pub async fn save_scope_message(
        &self,
        project_id: Option<i64>,
        scope_key: Option<&str>,
        role: &str,
        chunks_json: &str,
    ) -> ShepherdChatResult<i64> {
        self.save_message_with_project(project_id, scope_key, role, chunks_json)
            .await
    }

    pub async fn get_scope_messages(
        &self,
        project_id: Option<i64>,
        scope_key: Option<&str>,
        limit: usize,
    ) -> ShepherdChatResult<Vec<ShepherdChatMessage>> {
        let db = self.db().await;
        let lookup_key = chat_lookup_key(project_id, scope_key);
        let mut result = db
            .query(
                "SELECT * FROM shepherd_chat_message WHERE lookup_key = $lookup_key ORDER BY timestamp DESC LIMIT $limit",
            )
            .bind(("lookup_key", lookup_key))
            .bind(("limit", limit as i64))
            .await?;
        let mut records: Vec<ShepherdChatMessageRecord> = result.take(0)?;
        records.reverse();
        Ok(records
            .into_iter()
            .map(ShepherdChatMessageRecord::into_message)
            .collect())
    }

    pub async fn get_messages(
        &self,
        scope_key: Option<&str>,
    ) -> ShepherdChatResult<Vec<ShepherdChatMessage>> {
        let limit = i64::MAX as usize;
        self.get_scope_messages(None, scope_key, limit).await
    }

    pub async fn delete_project_messages(&self, project_id: i64) -> ShepherdChatResult<()> {
        let db = self.db().await;

        let messages: Vec<ShepherdChatMessageRecord> =
            db.select(SHEPHERD_CHAT_MESSAGE_TABLE).await?;
        for message in messages {
            if message.project_id == Some(project_id) {
                let _: Option<ShepherdChatMessageRecord> = db
                    .delete((SHEPHERD_CHAT_MESSAGE_TABLE, message.message_id))
                    .await?;
            }
        }

        let live_turns: Vec<ShepherdLiveTurnRecord> = db.select(SHEPHERD_LIVE_TURN_TABLE).await?;
        for live_turn in live_turns {
            if live_turn.project_id == Some(project_id) {
                let _: Option<ShepherdLiveTurnRecord> = db
                    .delete((SHEPHERD_LIVE_TURN_TABLE, live_turn.lookup_key))
                    .await?;
            }
        }

        let scope_states: Vec<ShepherdScopeStateRecord> =
            db.select(SHEPHERD_SCOPE_STATE_TABLE).await?;
        for scope_state in scope_states {
            if scope_state.project_id == project_id {
                let _: Option<ShepherdScopeStateRecord> = db
                    .delete((SHEPHERD_SCOPE_STATE_TABLE, scope_state.lookup_key))
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn delete_scope_messages(&self, scope_key: &str) -> ShepherdChatResult<()> {
        let db = self.db().await;

        let messages: Vec<ShepherdChatMessageRecord> =
            db.select(SHEPHERD_CHAT_MESSAGE_TABLE).await?;
        for message in messages {
            if message.scope_key.as_deref() == Some(scope_key) {
                let _: Option<ShepherdChatMessageRecord> = db
                    .delete((SHEPHERD_CHAT_MESSAGE_TABLE, message.message_id))
                    .await?;
            }
        }

        let live_turns: Vec<ShepherdLiveTurnRecord> = db.select(SHEPHERD_LIVE_TURN_TABLE).await?;
        for live_turn in live_turns {
            if live_turn.scope_key == scope_key {
                let _: Option<ShepherdLiveTurnRecord> = db
                    .delete((SHEPHERD_LIVE_TURN_TABLE, live_turn.lookup_key))
                    .await?;
            }
        }

        let scope_states: Vec<ShepherdScopeStateRecord> =
            db.select(SHEPHERD_SCOPE_STATE_TABLE).await?;
        for scope_state in scope_states {
            if scope_state.scope_key == scope_key {
                let _: Option<ShepherdScopeStateRecord> = db
                    .delete((SHEPHERD_SCOPE_STATE_TABLE, scope_state.lookup_key))
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn get_scope_state(
        &self,
        project_id: i64,
        scope_key: &str,
    ) -> ShepherdChatResult<Option<String>> {
        let db = self.db().await;
        let lookup_key = required_scope_lookup_key(project_id, scope_key);
        let record: Option<ShepherdScopeStateRecord> =
            db.select((SHEPHERD_SCOPE_STATE_TABLE, lookup_key)).await?;
        Ok(record.map(|record| record.state_json))
    }

    pub async fn save_scope_state(
        &self,
        project_id: i64,
        scope_key: &str,
        state_json: &str,
    ) -> ShepherdChatResult<()> {
        let db = self.db().await;
        let lookup_key = required_scope_lookup_key(project_id, scope_key);
        let record = ShepherdScopeStateRecord {
            lookup_key: lookup_key.clone(),
            project_id,
            scope_key: scope_key.to_string(),
            state_json: state_json.to_string(),
            updated_at: utc_now(),
        };

        let _: Option<ShepherdScopeStateRecord> = db
            .upsert((SHEPHERD_SCOPE_STATE_TABLE, lookup_key))
            .content(record.clone())
            .await?;
        Ok(())
    }

    pub async fn clear_scope_state(
        &self,
        project_id: i64,
        scope_key: &str,
    ) -> ShepherdChatResult<()> {
        let db = self.db().await;
        let lookup_key = required_scope_lookup_key(project_id, scope_key);
        let _: Option<ShepherdScopeStateRecord> =
            db.delete((SHEPHERD_SCOPE_STATE_TABLE, lookup_key)).await?;
        Ok(())
    }

    pub async fn get_live_turn(
        &self,
        project_id: Option<i64>,
        scope_key: &str,
    ) -> ShepherdChatResult<Option<ShepherdLiveTurn>> {
        let db = self.db().await;
        let lookup_key = live_turn_lookup_key(project_id, scope_key);
        let record: Option<ShepherdLiveTurnRecord> =
            db.select((SHEPHERD_LIVE_TURN_TABLE, lookup_key)).await?;
        Ok(record.map(ShepherdLiveTurnRecord::into_live_turn))
    }

    pub async fn save_live_turn(
        &self,
        project_id: Option<i64>,
        scope_key: &str,
        role: &str,
        chunks_json: &str,
        status: &str,
        error: Option<&str>,
    ) -> ShepherdChatResult<()> {
        let db = self.db().await;
        let lookup_key = live_turn_lookup_key(project_id, scope_key);
        let now = utc_now();
        let existing: Option<ShepherdLiveTurnRecord> = db
            .select((SHEPHERD_LIVE_TURN_TABLE, lookup_key.clone()))
            .await?;

        let record = ShepherdLiveTurnRecord {
            lookup_key: lookup_key.clone(),
            project_id,
            scope_key: scope_key.to_string(),
            role: role.to_string(),
            chunks_json: chunks_json.to_string(),
            status: status.to_string(),
            error: error.map(ToOwned::to_owned),
            started_at: existing
                .as_ref()
                .map(|record| record.started_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
        };

        let _: Option<ShepherdLiveTurnRecord> = db
            .upsert((SHEPHERD_LIVE_TURN_TABLE, lookup_key))
            .content(record.clone())
            .await?;
        publish_activity_event(project_id, Some(scope_key));
        Ok(())
    }

    pub async fn clear_live_turn(
        &self,
        project_id: Option<i64>,
        scope_key: &str,
    ) -> ShepherdChatResult<()> {
        let db = self.db().await;
        let lookup_key = live_turn_lookup_key(project_id, scope_key);
        let _: Option<ShepherdLiveTurnRecord> =
            db.delete((SHEPHERD_LIVE_TURN_TABLE, lookup_key)).await?;
        publish_activity_event(project_id, Some(scope_key));
        Ok(())
    }

    pub async fn clear_all_live_turns(&self) -> ShepherdChatResult<usize> {
        let db = self.db().await;
        let live_turns: Vec<ShepherdLiveTurnRecord> = db.select(SHEPHERD_LIVE_TURN_TABLE).await?;
        let mut cleared = 0usize;

        for live_turn in live_turns {
            let _: Option<ShepherdLiveTurnRecord> = db
                .delete((SHEPHERD_LIVE_TURN_TABLE, live_turn.lookup_key.clone()))
                .await?;
            publish_activity_event(live_turn.project_id, Some(&live_turn.scope_key));
            cleared += 1;
        }

        Ok(cleared)
    }
}

impl ShepherdChatMessageRecord {
    fn into_message(self) -> ShepherdChatMessage {
        ShepherdChatMessage {
            id: self.message_id,
            project_id: self.project_id,
            scope_key: self.scope_key,
            role: self.role,
            timestamp: self.timestamp,
            chunks_json: self.chunks_json,
        }
    }
}

impl ShepherdLiveTurnRecord {
    fn into_live_turn(self) -> ShepherdLiveTurn {
        ShepherdLiveTurn {
            project_id: self.project_id,
            scope_key: self.scope_key,
            role: self.role,
            chunks_json: self.chunks_json,
            status: self.status,
            error: self.error,
            started_at: self.started_at,
            updated_at: self.updated_at,
        }
    }
}

fn chat_lookup_key(project_id: Option<i64>, scope_key: Option<&str>) -> String {
    format!(
        "project:{}|scope:{}",
        project_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "none".to_string()),
        scope_key.unwrap_or("none")
    )
}

fn required_scope_lookup_key(project_id: i64, scope_key: &str) -> String {
    format!("project:{project_id}|scope:{scope_key}")
}

fn live_turn_lookup_key(project_id: Option<i64>, scope_key: &str) -> String {
    format!(
        "project:{}|scope:{scope_key}",
        project_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "none".to_string())
    )
}

fn publish_history_event(project_id: Option<i64>, scope_key: Option<&str>) {
    let Some(project_id) = live_updates::scope_project_id(project_id) else {
        return;
    };
    if let Some(thread_id) = live_updates::scope_thread_id(scope_key) {
        live_updates::publish_thread(
            project_id,
            thread_id.to_string(),
            LiveUpdateKind::ThreadHistoryChanged,
        );
    } else if live_updates::scope_is_librarian(scope_key) {
        live_updates::publish_project(project_id, LiveUpdateKind::LibrarianHistoryChanged);
    } else {
        live_updates::publish_project(project_id, LiveUpdateKind::ProjectHistoryChanged);
    }
}

fn publish_activity_event(project_id: Option<i64>, scope_key: Option<&str>) {
    let Some(project_id) = live_updates::scope_project_id(project_id) else {
        return;
    };
    if let Some(thread_id) = live_updates::scope_thread_id(scope_key) {
        live_updates::publish_thread(
            project_id,
            thread_id.to_string(),
            LiveUpdateKind::ThreadActivityChanged,
        );
    } else if live_updates::scope_is_librarian(scope_key) {
        live_updates::publish_project(project_id, LiveUpdateKind::LibrarianActivityChanged);
    } else {
        live_updates::publish_project(project_id, LiveUpdateKind::ProjectActivityChanged);
    }
}

#[cfg(test)]
mod tests {}
