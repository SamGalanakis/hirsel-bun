use serde::{Deserialize, Serialize};
use surrealdb::types::SurrealValue;

use crate::backend::db::{global_db, utc_now, DbClient};
use crate::backend::live_updates::{self, LiveUpdateKind};

const TASK_TABLE: &str = "task";

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
struct TaskRecord {
    task_id: String,
    project_id: i64,
    title: String,
    status: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    review_json: Option<String>,
    #[serde(default)]
    sort_order: i64,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub project_id: i64,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub review_json: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("Database error: {0}")]
    Database(#[from] surrealdb::Error),
    #[error("Task not found: {0}")]
    NotFound(String),
}

pub type TaskResult<T> = Result<T, TaskError>;

pub struct TaskStore;

impl TaskStore {
    pub async fn open() -> Result<Self, surrealdb::Error> {
        let _ = global_db().await;
        Ok(Self)
    }

    async fn db(&self) -> &'static DbClient {
        global_db().await
    }

    pub async fn create_task(
        &self,
        project_id: i64,
        title: &str,
        content: Option<&str>,
    ) -> TaskResult<Task> {
        let db = self.db().await;
        let id = uuid::Uuid::new_v4().to_string();
        let now = utc_now();

        // Get next sort_order
        let mut response = db
            .query("SELECT sort_order FROM task WHERE project_id = $pid ORDER BY sort_order DESC LIMIT 1")
            .bind(("pid", project_id))
            .await?;
        let max_order: Option<TaskRecord> = response
            .take(0)
            .ok()
            .and_then(|v: Vec<TaskRecord>| v.into_iter().next());
        let next_order = max_order.map(|r| r.sort_order + 1).unwrap_or(0);

        let record = TaskRecord {
            task_id: id.clone(),
            project_id,
            title: title.to_string(),
            status: "todo".to_string(),
            content: content.map(ToOwned::to_owned),
            review_json: None,
            sort_order: next_order,
            created_at: now.clone(),
            updated_at: now,
        };

        let _: Option<TaskRecord> = db
            .create((TASK_TABLE, id.as_str()))
            .content(record.clone())
            .await?;

        live_updates::publish_project(project_id, LiveUpdateKind::TasksChanged);

        // Sync to knowledge graph
        let _ = self.sync_task_to_kg(project_id, &id, title, content).await;

        Ok(record.into_task())
    }

    pub async fn get_task(&self, task_id: &str) -> TaskResult<Task> {
        let record: Option<TaskRecord> = self.db().await.select((TASK_TABLE, task_id)).await?;
        record
            .map(TaskRecord::into_task)
            .ok_or_else(|| TaskError::NotFound(task_id.to_string()))
    }

    pub async fn list_project_tasks(&self, project_id: i64) -> TaskResult<Vec<Task>> {
        let db = self.db().await;
        let mut response = db
            .query("SELECT * FROM task WHERE project_id = $pid ORDER BY sort_order ASC")
            .bind(("pid", project_id))
            .await?;
        let records: Vec<TaskRecord> = response.take(0).unwrap_or_default();
        Ok(records.into_iter().map(TaskRecord::into_task).collect())
    }

    pub async fn update_task(
        &self,
        task_id: &str,
        title: Option<&str>,
        status: Option<&str>,
        content: Option<Option<&str>>,
    ) -> TaskResult<Task> {
        let db = self.db().await;
        let mut record: TaskRecord = db
            .select((TASK_TABLE, task_id))
            .await?
            .ok_or_else(|| TaskError::NotFound(task_id.to_string()))?;

        if let Some(title) = title {
            record.title = title.to_string();
        }
        if let Some(status) = status {
            record.status = status.to_string();
        }
        if let Some(content) = content {
            record.content = content.map(ToOwned::to_owned);
        }
        record.updated_at = utc_now();

        let _: Option<TaskRecord> = db
            .upsert((TASK_TABLE, task_id))
            .content(record.clone())
            .await?;

        let project_id = record.project_id;
        live_updates::publish_project(project_id, LiveUpdateKind::TaskChanged);

        // Sync to knowledge graph
        let _ = self
            .sync_task_to_kg(
                project_id,
                task_id,
                &record.title,
                record.content.as_deref(),
            )
            .await;

        Ok(record.into_task())
    }

    pub async fn update_task_content(&self, task_id: &str, content: &str) -> TaskResult<Task> {
        self.update_task(task_id, None, None, Some(Some(content)))
            .await
    }

    pub async fn set_review(&self, task_id: &str, review_json: &str) -> TaskResult<Task> {
        let db = self.db().await;
        let mut record: TaskRecord = db
            .select((TASK_TABLE, task_id))
            .await?
            .ok_or_else(|| TaskError::NotFound(task_id.to_string()))?;

        record.review_json = Some(review_json.to_string());
        record.status = "review".to_string();
        record.updated_at = utc_now();

        let _: Option<TaskRecord> = db
            .upsert((TASK_TABLE, task_id))
            .content(record.clone())
            .await?;

        live_updates::publish_project(record.project_id, LiveUpdateKind::TaskChanged);
        Ok(record.into_task())
    }

    pub async fn delete_task(&self, task_id: &str) -> TaskResult<()> {
        let db = self.db().await;
        let record: Option<TaskRecord> = db.select((TASK_TABLE, task_id)).await?;
        let project_id = record
            .as_ref()
            .map(|r| r.project_id)
            .ok_or_else(|| TaskError::NotFound(task_id.to_string()))?;

        let _: Option<TaskRecord> = db.delete((TASK_TABLE, task_id)).await?;
        live_updates::publish_project(project_id, LiveUpdateKind::TasksChanged);
        Ok(())
    }

    pub async fn reorder_tasks(&self, project_id: i64, task_ids: &[String]) -> TaskResult<()> {
        let db = self.db().await;
        for (idx, task_id) in task_ids.iter().enumerate() {
            let tid = task_id.clone();
            let _ = db
                .query(
                    "UPDATE type::record('task', $tid) SET sort_order = $order, updated_at = $now",
                )
                .bind(("tid", tid))
                .bind(("order", idx as i64))
                .bind(("now", utc_now()))
                .await;
        }
        live_updates::publish_project(project_id, LiveUpdateKind::TasksChanged);
        Ok(())
    }

    async fn sync_task_to_kg(
        &self,
        project_id: i64,
        task_id: &str,
        title: &str,
        content: Option<&str>,
    ) -> Result<(), String> {
        let db = self.db().await;
        let tid = task_id.to_string();
        let title_owned = title.to_string();
        let content_owned = content.unwrap_or("").to_string();
        let _ = db
            .query(
                "UPSERT type::record('kg_node', [$pid, 'task', $tid]) MERGE {
                    project_id: $pid,
                    kind: 'task',
                    node_id: $tid,
                    label: $title,
                    content: $content,
                    source: 'user',
                    tags: ['task'],
                }",
            )
            .bind(("pid", project_id))
            .bind(("tid", tid))
            .bind(("title", title_owned))
            .bind(("content", content_owned))
            .await
            .map_err(|e| format!("failed to sync task to kg: {e}"))?;
        Ok(())
    }
}

impl TaskRecord {
    fn into_task(self) -> Task {
        Task {
            id: self.task_id,
            project_id: self.project_id,
            title: self.title,
            status: self.status,
            content: self.content,
            review_json: self.review_json,
            sort_order: self.sort_order,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}
