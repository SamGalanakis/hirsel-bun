use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use crate::backend::db::{global_db, utc_now, DbClient};
use crate::backend::live_updates::{self, LiveUpdateKind};

const SHEPHERD_THREAD_TABLE: &str = "shepherd_thread";

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct ShepherdThreadRecord {
    thread_id: String,
    project_id: i64,
    title: String,
    title_lower: String,
    objective: String,
    summary: String,
    status: String,
    workspace_path: Option<String>,
    checkout_name: Option<String>,
    created_at: String,
    updated_at: String,
    last_activity_at: String,
    archived_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdThread {
    pub id: String,
    pub project_id: i64,
    pub title: String,
    pub objective: String,
    pub summary: String,
    pub status: String,
    pub workspace_path: Option<String>,
    pub checkout_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_activity_at: String,
    pub archived_at: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ShepherdThreadError {
    #[error("Database error: {0}")]
    Database(#[from] surrealdb::Error),
    #[error("Thread not found: {0}")]
    NotFound(String),
}

pub type ShepherdThreadResult<T> = Result<T, ShepherdThreadError>;

pub struct ShepherdThreadStore;

impl ShepherdThreadStore {
    pub fn scope_key(thread_id: &str) -> String {
        format!("__thread__:{thread_id}")
    }

    pub async fn open() -> ShepherdThreadResult<Self> {
        let _ = global_db().await;
        Ok(Self)
    }

    async fn db(&self) -> &'static DbClient {
        global_db().await
    }

    pub async fn create_thread(
        &self,
        project_id: i64,
        title: &str,
        objective: &str,
        summary: &str,
        workspace_path: Option<&str>,
        checkout_name: Option<&str>,
    ) -> ShepherdThreadResult<ShepherdThread> {
        let db = self.db().await;
        let id = uuid::Uuid::new_v4().to_string();
        let now = utc_now();
        let record = ShepherdThreadRecord {
            thread_id: id.clone(),
            project_id,
            title: title.to_string(),
            title_lower: normalize_text(title),
            objective: objective.to_string(),
            summary: summary.to_string(),
            status: "running".to_string(),
            workspace_path: workspace_path.map(ToOwned::to_owned),
            checkout_name: checkout_name.map(ToOwned::to_owned),
            created_at: now.clone(),
            updated_at: now.clone(),
            last_activity_at: now,
            archived_at: None,
        };

        let _: Option<ShepherdThreadRecord> = db
            .create((SHEPHERD_THREAD_TABLE, id.clone()))
            .content(record.clone())
            .await?;
        live_updates::publish_project(project_id, LiveUpdateKind::ThreadsChanged);
        live_updates::publish_thread(project_id, id.clone(), LiveUpdateKind::ThreadChanged);

        Ok(record.into_thread())
    }

    pub async fn get_thread(&self, thread_id: &str) -> ShepherdThreadResult<ShepherdThread> {
        let db = self.db().await;
        let record: Option<ShepherdThreadRecord> =
            db.select((SHEPHERD_THREAD_TABLE, thread_id)).await?;
        record
            .map(ShepherdThreadRecord::into_thread)
            .ok_or_else(|| ShepherdThreadError::NotFound(thread_id.to_string()))
    }

    pub async fn list_project_threads(
        &self,
        project_id: i64,
    ) -> ShepherdThreadResult<Vec<ShepherdThread>> {
        let db = self.db().await;
        let mut result = db
            .query(
                "SELECT * FROM shepherd_thread WHERE project_id = $project_id AND archived_at = NONE",
            )
            .bind(("project_id", project_id))
            .await?;
        let mut records: Vec<ShepherdThreadRecord> = result.take(0)?;
        records.sort_by(|a, b| {
            thread_status_rank(&a.status)
                .cmp(&thread_status_rank(&b.status))
                .then_with(|| b.last_activity_at.cmp(&a.last_activity_at))
                .then_with(|| b.created_at.cmp(&a.created_at))
        });

        Ok(records
            .into_iter()
            .map(ShepherdThreadRecord::into_thread)
            .collect())
    }

    pub async fn find_project_thread_by_title(
        &self,
        project_id: i64,
        title: &str,
    ) -> ShepherdThreadResult<Option<ShepherdThread>> {
        let title_lower = normalize_text(title);
        let db = self.db().await;
        let mut result = db
            .query(
                "SELECT * FROM shepherd_thread WHERE project_id = $project_id AND title_lower = $title_lower AND archived_at = NONE ORDER BY updated_at DESC LIMIT 1",
            )
            .bind(("project_id", project_id))
            .bind(("title_lower", title_lower))
            .await?;
        let record: Option<ShepherdThreadRecord> = result.take(0)?;
        Ok(record.map(ShepherdThreadRecord::into_thread))
    }

    pub async fn update_thread(
        &self,
        thread_id: &str,
        title: Option<&str>,
        objective: Option<&str>,
        summary: Option<&str>,
        status: Option<&str>,
        workspace_path: Option<Option<&str>>,
        checkout_name: Option<Option<&str>>,
    ) -> ShepherdThreadResult<()> {
        let db = self.db().await;
        let mut record = self
            .load_thread_record(thread_id)
            .await?
            .ok_or_else(|| ShepherdThreadError::NotFound(thread_id.to_string()))?;

        if let Some(title) = title {
            record.title = title.to_string();
            record.title_lower = normalize_text(title);
        }
        if let Some(objective) = objective {
            record.objective = objective.to_string();
        }
        if let Some(summary) = summary {
            record.summary = summary.to_string();
        }
        if let Some(status) = status {
            record.status = status.to_string();
        }
        if let Some(workspace_path) = workspace_path {
            record.workspace_path = workspace_path.map(ToOwned::to_owned);
        }
        if let Some(checkout_name) = checkout_name {
            record.checkout_name = checkout_name.map(ToOwned::to_owned);
        }

        let now = utc_now();
        record.updated_at = now.clone();
        record.last_activity_at = now;

        let _: Option<ShepherdThreadRecord> = db
            .upsert((SHEPHERD_THREAD_TABLE, thread_id))
            .content(record.clone())
            .await?;
        live_updates::publish_project(record.project_id, LiveUpdateKind::ThreadsChanged);
        live_updates::publish_thread(
            record.project_id,
            record.thread_id.clone(),
            LiveUpdateKind::ThreadChanged,
        );
        Ok(())
    }

    pub async fn touch_thread(&self, thread_id: &str) -> ShepherdThreadResult<()> {
        let db = self.db().await;
        if let Some(mut record) = self.load_thread_record(thread_id).await? {
            let now = utc_now();
            record.updated_at = now.clone();
            record.last_activity_at = now;
            let _: Option<ShepherdThreadRecord> = db
                .upsert((SHEPHERD_THREAD_TABLE, thread_id))
                .content(record.clone())
                .await?;
            live_updates::publish_project(record.project_id, LiveUpdateKind::ThreadsChanged);
            live_updates::publish_thread(
                record.project_id,
                record.thread_id.clone(),
                LiveUpdateKind::ThreadChanged,
            );
        }
        Ok(())
    }

    pub async fn archive_thread(&self, thread_id: &str) -> ShepherdThreadResult<()> {
        let db = self.db().await;
        if let Some(mut record) = self.load_thread_record(thread_id).await? {
            let now = utc_now();
            record.archived_at = Some(now.clone());
            record.updated_at = now;
            let _: Option<ShepherdThreadRecord> = db
                .upsert((SHEPHERD_THREAD_TABLE, thread_id))
                .content(record.clone())
                .await?;
            live_updates::publish_project(record.project_id, LiveUpdateKind::ThreadsChanged);
            live_updates::publish_thread(
                record.project_id,
                record.thread_id.clone(),
                LiveUpdateKind::ThreadChanged,
            );
        }
        Ok(())
    }

    pub async fn delete_thread(&self, thread_id: &str) -> ShepherdThreadResult<()> {
        let db = self.db().await;
        if let Some(record) = self.load_thread_record(thread_id).await? {
            let _: Option<ShepherdThreadRecord> =
                db.delete((SHEPHERD_THREAD_TABLE, thread_id)).await?;
            live_updates::publish_project(record.project_id, LiveUpdateKind::ThreadsChanged);
            live_updates::publish_thread(
                record.project_id,
                record.thread_id,
                LiveUpdateKind::ThreadChanged,
            );
        }
        Ok(())
    }

    pub async fn delete_project_threads(&self, project_id: i64) -> ShepherdThreadResult<()> {
        let db = self.db().await;
        let records: Vec<ShepherdThreadRecord> = db.select(SHEPHERD_THREAD_TABLE).await?;
        for record in records {
            if record.project_id == project_id {
                let _: Option<ShepherdThreadRecord> =
                    db.delete((SHEPHERD_THREAD_TABLE, record.thread_id)).await?;
            }
        }
        live_updates::publish_project(project_id, LiveUpdateKind::ThreadsChanged);
        Ok(())
    }

    async fn load_thread_record(
        &self,
        thread_id: &str,
    ) -> ShepherdThreadResult<Option<ShepherdThreadRecord>> {
        let db = self.db().await;
        Ok(db.select((SHEPHERD_THREAD_TABLE, thread_id)).await?)
    }
}

impl ShepherdThreadRecord {
    fn into_thread(self) -> ShepherdThread {
        ShepherdThread {
            id: self.thread_id,
            project_id: self.project_id,
            title: self.title,
            objective: self.objective,
            summary: self.summary,
            status: self.status,
            workspace_path: self.workspace_path,
            checkout_name: self.checkout_name,
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_activity_at: self.last_activity_at,
            archived_at: self.archived_at,
        }
    }
}

fn normalize_text(value: &str) -> String {
    value.to_lowercase()
}

fn thread_status_rank(status: &str) -> u8 {
    match status {
        "running" => 0,
        "waiting" => 1,
        "blocked" => 2,
        "failed" => 3,
        "done" => 4,
        _ => 5,
    }
}
