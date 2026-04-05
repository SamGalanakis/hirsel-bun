use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use crate::backend::db::{global_db, utc_now, DbClient};
use crate::backend::live_updates::{self, LiveUpdateKind};

const SHEPHERD_SESSION_TABLE: &str = "shepherd_session";

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdScopeSession {
    pub project_id: Option<i64>,
    pub scope_key: String,
    pub scope_json: String,
    pub workspace_path: Option<String>,
    pub env_fingerprint: Option<String>,
    pub runtime_fingerprint: Option<String>,
    pub status: String,
    pub container_name: Option<String>,
    pub socket_path: String,
    pub bootstrap_flake: bool,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ShepherdSessionError {
    #[error("Database error: {0}")]
    Database(#[from] surrealdb::Error),
}

pub type ShepherdSessionResult<T> = Result<T, ShepherdSessionError>;

pub struct ShepherdSessionStore;

impl ShepherdSessionStore {
    pub async fn open() -> ShepherdSessionResult<Self> {
        let _ = global_db().await;
        Ok(Self)
    }

    async fn db(&self) -> &'static DbClient {
        global_db().await
    }

    pub async fn get_session(
        &self,
        scope_key: &str,
    ) -> ShepherdSessionResult<Option<ShepherdScopeSession>> {
        let db = self.db().await;
        Ok(db.select((SHEPHERD_SESSION_TABLE, scope_key)).await?)
    }

    pub async fn list_sessions(&self) -> ShepherdSessionResult<Vec<ShepherdScopeSession>> {
        let db = self.db().await;
        Ok(db.select(SHEPHERD_SESSION_TABLE).await?)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_session(
        &self,
        project_id: Option<i64>,
        scope_key: &str,
        scope_json: &str,
        workspace_path: Option<&str>,
        env_fingerprint: Option<&str>,
        runtime_fingerprint: Option<&str>,
        socket_path: &str,
        bootstrap_flake: bool,
        container_name: Option<&str>,
        status: &str,
        last_error: Option<&str>,
    ) -> ShepherdSessionResult<()> {
        let db = self.db().await;
        let existing = self.get_session(scope_key).await?;
        let now = utc_now();
        let record = ShepherdScopeSession {
            project_id,
            scope_key: scope_key.to_string(),
            scope_json: scope_json.to_string(),
            workspace_path: workspace_path.map(ToOwned::to_owned),
            env_fingerprint: env_fingerprint.map(ToOwned::to_owned),
            runtime_fingerprint: runtime_fingerprint.map(ToOwned::to_owned),
            status: status.to_string(),
            container_name: container_name.map(ToOwned::to_owned),
            socket_path: socket_path.to_string(),
            bootstrap_flake,
            last_error: last_error.map(ToOwned::to_owned),
            created_at: existing
                .as_ref()
                .map(|session| session.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
            last_seen_at: existing.and_then(|session| session.last_seen_at),
        };

        let _: Option<ShepherdScopeSession> = db
            .upsert((SHEPHERD_SESSION_TABLE, scope_key))
            .content(record.clone())
            .await?;
        publish_session_activity(record.project_id, &record.scope_key);
        Ok(())
    }

    pub async fn set_status(
        &self,
        scope_key: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> ShepherdSessionResult<()> {
        let db = self.db().await;
        if let Some(mut session) = self.get_session(scope_key).await? {
            session.status = status.to_string();
            session.last_error = last_error.map(ToOwned::to_owned);
            session.updated_at = utc_now();

            let _: Option<ShepherdScopeSession> = db
                .upsert((SHEPHERD_SESSION_TABLE, scope_key))
                .content(session.clone())
                .await?;
            publish_session_activity(session.project_id, &session.scope_key);
        }
        Ok(())
    }

    pub async fn touch_seen(&self, scope_key: &str) -> ShepherdSessionResult<()> {
        let db = self.db().await;
        if let Some(mut session) = self.get_session(scope_key).await? {
            let now = utc_now();
            session.last_seen_at = Some(now.clone());
            session.updated_at = now;

            let _: Option<ShepherdScopeSession> = db
                .upsert((SHEPHERD_SESSION_TABLE, scope_key))
                .content(session.clone())
                .await?;
            publish_session_activity(session.project_id, &session.scope_key);
        }
        Ok(())
    }

    pub async fn delete_session(&self, scope_key: &str) -> ShepherdSessionResult<()> {
        let db = self.db().await;
        if let Some(session) = self.get_session(scope_key).await? {
            let _: Option<ShepherdScopeSession> =
                db.delete((SHEPHERD_SESSION_TABLE, scope_key)).await?;
            publish_session_activity(session.project_id, &session.scope_key);
        }
        Ok(())
    }

    pub async fn clear_stale_startup_state(&self) -> ShepherdSessionResult<usize> {
        let db = self.db().await;
        let mut cleared = 0usize;

        for mut session in self.list_sessions().await? {
            let mut changed = false;

            if session.last_error.is_some() {
                session.last_error = None;
                changed = true;
            }

            if session.status != "idle" {
                session.status = "idle".to_string();
                changed = true;
            }

            if !changed {
                continue;
            }

            session.updated_at = utc_now();
            let _: Option<ShepherdScopeSession> = db
                .upsert((SHEPHERD_SESSION_TABLE, session.scope_key.clone()))
                .content(session.clone())
                .await?;
            publish_session_activity(session.project_id, &session.scope_key);
            cleared += 1;
        }

        Ok(cleared)
    }
}

fn publish_session_activity(project_id: Option<i64>, scope_key: &str) {
    let Some(project_id) = live_updates::scope_project_id(project_id) else {
        return;
    };
    if let Some(thread_id) = live_updates::scope_thread_id(Some(scope_key)) {
        live_updates::publish_thread(
            project_id,
            thread_id.to_string(),
            LiveUpdateKind::ThreadActivityChanged,
        );
    } else if live_updates::scope_is_librarian(Some(scope_key)) {
        live_updates::publish_project(project_id, LiveUpdateKind::LibrarianActivityChanged);
    } else {
        live_updates::publish_project(project_id, LiveUpdateKind::ProjectActivityChanged);
    }
}
