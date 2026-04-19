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
    #[serde(default)]
    workspace_path: Option<String>,
    created_at: String,
    updated_at: String,
    last_activity_at: String,
    archived_at: Option<String>,
    #[serde(default)]
    highlight: Option<String>,
    #[serde(default)]
    focused_task_id: Option<String>,
    // Unified-thread columns (phase 1, additive).
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default = "default_binding_kind")]
    binding_kind: String,
    #[serde(default)]
    binding_data: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    sort_order: i64,
    #[serde(default)]
    final_output: Option<String>,
    #[serde(default = "default_merge_status")]
    merge_status: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    review_json: Option<String>,
}

fn default_binding_kind() -> String {
    "free".to_string()
}

fn default_merge_status() -> String {
    "none".to_string()
}

pub const BINDING_KIND_FREE: &str = "free";
pub const BINDING_KIND_TASK: &str = "task";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdThread {
    pub id: String,
    pub project_id: i64,
    pub title: String,
    pub objective: String,
    pub summary: String,
    pub status: String,
    pub cwd: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_activity_at: String,
    pub archived_at: Option<String>,
    #[serde(default)]
    pub highlight: Option<String>,
    #[serde(default)]
    pub focused_task_id: Option<String>,
    // Unified-thread columns (phase 1, additive). Optional in the API
    // surface today; tools and UI will start populating them in later
    // tasks within this phase.
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default = "default_binding_kind")]
    pub binding_kind: String,
    #[serde(default)]
    pub binding_data: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub final_output: Option<String>,
    #[serde(default = "default_merge_status")]
    pub merge_status: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub review_json: Option<String>,
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
}

impl ShepherdThreadStore {
    pub async fn open() -> ShepherdThreadResult<Self> {
        let _ = global_db().await;
        Ok(Self)
    }

    async fn db(&self) -> &'static DbClient {
        global_db().await
    }

    /// Configuration for a new thread spawned via `spawn_thread` (or any
    /// caller that needs to set the unified-thread columns up-front).
    pub async fn create_thread_with_config(
        &self,
        project_id: i64,
        title: &str,
        objective: &str,
        summary: &str,
        cwd: Option<&str>,
        parent_id: Option<String>,
        capabilities: Vec<String>,
        binding_kind: String,
        binding_data: Option<String>,
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
            workspace_path: cwd.map(ToOwned::to_owned),
            created_at: now.clone(),
            updated_at: now.clone(),
            last_activity_at: now,
            archived_at: None,
            highlight: None,
            focused_task_id: None,
            parent_id,
            binding_kind,
            binding_data,
            capabilities,
            sort_order: 0,
            final_output: None,
            merge_status: default_merge_status(),
            content: None,
            review_json: None,
        };
        let _: Option<ShepherdThreadRecord> = db
            .create((SHEPHERD_THREAD_TABLE, id.as_str()))
            .content(record.clone())
            .await?;
        live_updates::publish_project(project_id, LiveUpdateKind::ThreadsChanged);
        live_updates::publish_thread(project_id, id.clone(), LiveUpdateKind::ThreadChanged);
        Ok(record.into_thread())
    }

    pub async fn set_thread_workspace_path(
        &self,
        thread_id: &str,
        workspace_path: Option<&str>,
    ) -> ShepherdThreadResult<()> {
        let db = self.db().await;
        let mut record = self
            .load_thread_record(thread_id)
            .await?
            .ok_or_else(|| ShepherdThreadError::NotFound(thread_id.to_string()))?;
        record.workspace_path = workspace_path.map(ToOwned::to_owned);
        record.updated_at = utc_now();
        let _: Option<ShepherdThreadRecord> = db
            .upsert((SHEPHERD_THREAD_TABLE, thread_id))
            .content(record.clone())
            .await?;
        Ok(())
    }

    pub async fn set_thread_merge_status(
        &self,
        thread_id: &str,
        merge_status: &str,
    ) -> ShepherdThreadResult<()> {
        let db = self.db().await;
        let mut record = self
            .load_thread_record(thread_id)
            .await?
            .ok_or_else(|| ShepherdThreadError::NotFound(thread_id.to_string()))?;
        record.merge_status = merge_status.to_string();
        record.updated_at = utc_now();
        let _: Option<ShepherdThreadRecord> = db
            .upsert((SHEPHERD_THREAD_TABLE, thread_id))
            .content(record.clone())
            .await?;
        live_updates::publish_thread(
            record.project_id,
            thread_id.to_string(),
            LiveUpdateKind::ThreadChanged,
        );
        Ok(())
    }

    pub async fn create_thread(
        &self,
        project_id: i64,
        title: &str,
        objective: &str,
        summary: &str,
        cwd: Option<&str>,
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
            workspace_path: cwd.map(ToOwned::to_owned),
            created_at: now.clone(),
            updated_at: now.clone(),
            last_activity_at: now,
            archived_at: None,
            highlight: None,
            focused_task_id: None,
            parent_id: None,
            binding_kind: default_binding_kind(),
            binding_data: None,
            capabilities: Vec::new(),
            sort_order: 0,
            final_output: None,
            merge_status: default_merge_status(),
            content: None,
            review_json: None,
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
        cwd: Option<Option<&str>>,
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
        if let Some(cwd) = cwd {
            record.workspace_path = cwd.map(ToOwned::to_owned);
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

    pub async fn set_highlight(
        &self,
        thread_id: &str,
        highlight: Option<&str>,
    ) -> ShepherdThreadResult<()> {
        let db = self.db().await;
        if let Some(mut record) = self.load_thread_record(thread_id).await? {
            record.highlight = highlight.map(ToOwned::to_owned);
            let _: Option<ShepherdThreadRecord> = db
                .upsert((SHEPHERD_THREAD_TABLE, thread_id))
                .content(record.clone())
                .await?;
            live_updates::publish_thread(
                record.project_id,
                record.thread_id.clone(),
                LiveUpdateKind::ThreadChanged,
            );
        }
        Ok(())
    }

    pub async fn set_focused_task(
        &self,
        thread_id: &str,
        task_id: Option<&str>,
    ) -> ShepherdThreadResult<()> {
        let db = self.db().await;
        if let Some(mut record) = self.load_thread_record(thread_id).await? {
            record.focused_task_id = task_id.map(ToOwned::to_owned);
            let _: Option<ShepherdThreadRecord> = db
                .upsert((SHEPHERD_THREAD_TABLE, thread_id))
                .content(record.clone())
                .await?;
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

    // ─────────────────────────────────────────────────────────────────────
    // Task-as-thread operations
    //
    // "Tasks" are threads with `binding_kind = "task"`. They're the
    // board/planning entities the UI surfaces as a task list. The methods
    // below operate on that subset without duplicating storage.
    // ─────────────────────────────────────────────────────────────────────

    pub async fn create_task_thread(
        &self,
        project_id: i64,
        title: &str,
        content: Option<&str>,
    ) -> ShepherdThreadResult<ShepherdThread> {
        let db = self.db().await;
        let id = uuid::Uuid::new_v4().to_string();
        let now = utc_now();

        let mut response = db
            .query(
                "SELECT sort_order FROM shepherd_thread \
                 WHERE project_id = $pid AND binding_kind = 'task' \
                 ORDER BY sort_order DESC LIMIT 1",
            )
            .bind(("pid", project_id))
            .await?;
        let max: Option<ShepherdThreadRecord> = response
            .take(0)
            .ok()
            .and_then(|v: Vec<ShepherdThreadRecord>| v.into_iter().next());
        let next_order = max.map(|r| r.sort_order + 1).unwrap_or(0);

        let record = ShepherdThreadRecord {
            thread_id: id.clone(),
            project_id,
            title: title.to_string(),
            title_lower: normalize_text(title),
            objective: String::new(),
            summary: String::new(),
            status: "todo".to_string(),
            workspace_path: None,
            created_at: now.clone(),
            updated_at: now.clone(),
            last_activity_at: now,
            archived_at: None,
            highlight: None,
            focused_task_id: None,
            parent_id: None,
            binding_kind: BINDING_KIND_TASK.to_string(),
            binding_data: None,
            capabilities: Vec::new(),
            sort_order: next_order,
            final_output: None,
            merge_status: default_merge_status(),
            content: content.map(ToOwned::to_owned),
            review_json: None,
        };

        let _: Option<ShepherdThreadRecord> = db
            .create((SHEPHERD_THREAD_TABLE, id.as_str()))
            .content(record.clone())
            .await?;
        live_updates::publish_project(project_id, LiveUpdateKind::ThreadsChanged);
        sync_task_thread_to_kg(db, project_id, &id, title, content).await;
        Ok(record.into_thread())
    }

    pub async fn list_project_task_threads(
        &self,
        project_id: i64,
    ) -> ShepherdThreadResult<Vec<ShepherdThread>> {
        let db = self.db().await;
        let mut response = db
            .query(
                "SELECT * FROM shepherd_thread \
                 WHERE project_id = $pid AND binding_kind = 'task' \
                 ORDER BY sort_order ASC",
            )
            .bind(("pid", project_id))
            .await?;
        let records: Vec<ShepherdThreadRecord> = response.take(0).unwrap_or_default();
        Ok(records
            .into_iter()
            .map(ShepherdThreadRecord::into_thread)
            .collect())
    }

    pub async fn update_task_thread_fields(
        &self,
        thread_id: &str,
        title: Option<&str>,
        status: Option<&str>,
        content: Option<Option<&str>>,
    ) -> ShepherdThreadResult<ShepherdThread> {
        let db = self.db().await;
        let mut record = self
            .load_thread_record(thread_id)
            .await?
            .ok_or_else(|| ShepherdThreadError::NotFound(thread_id.to_string()))?;

        if let Some(title) = title {
            record.title = title.to_string();
            record.title_lower = normalize_text(title);
        }
        if let Some(status) = status {
            record.status = status.to_string();
        }
        if let Some(content) = content {
            record.content = content.map(ToOwned::to_owned);
        }
        record.updated_at = utc_now();

        let _: Option<ShepherdThreadRecord> = db
            .upsert((SHEPHERD_THREAD_TABLE, thread_id))
            .content(record.clone())
            .await?;

        let project_id = record.project_id;
        live_updates::publish_project(project_id, LiveUpdateKind::TaskChanged);
        sync_task_thread_to_kg(
            db,
            project_id,
            thread_id,
            &record.title,
            record.content.as_deref(),
        )
        .await;

        Ok(record.into_thread())
    }

    pub async fn set_task_thread_review(
        &self,
        thread_id: &str,
        review_json: &str,
    ) -> ShepherdThreadResult<ShepherdThread> {
        let db = self.db().await;
        let mut record = self
            .load_thread_record(thread_id)
            .await?
            .ok_or_else(|| ShepherdThreadError::NotFound(thread_id.to_string()))?;

        record.review_json = Some(review_json.to_string());
        record.status = "review".to_string();
        record.updated_at = utc_now();

        let _: Option<ShepherdThreadRecord> = db
            .upsert((SHEPHERD_THREAD_TABLE, thread_id))
            .content(record.clone())
            .await?;

        live_updates::publish_project(record.project_id, LiveUpdateKind::TaskChanged);
        Ok(record.into_thread())
    }

    pub async fn reorder_task_threads(
        &self,
        project_id: i64,
        ordered_ids: &[String],
    ) -> ShepherdThreadResult<()> {
        let db = self.db().await;
        for (idx, thread_id) in ordered_ids.iter().enumerate() {
            let tid = thread_id.clone();
            let _ = db
                .query(
                    "UPDATE type::record('shepherd_thread', $tid) \
                     SET sort_order = $order, updated_at = $now",
                )
                .bind(("tid", tid))
                .bind(("order", idx as i64))
                .bind(("now", utc_now()))
                .await;
        }
        live_updates::publish_project(project_id, LiveUpdateKind::TasksChanged);
        Ok(())
    }
}

async fn sync_task_thread_to_kg(
    db: &DbClient,
    project_id: i64,
    thread_id: &str,
    title: &str,
    content: Option<&str>,
) {
    let tid = thread_id.to_string();
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
        .await;
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
            cwd: self.workspace_path,
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_activity_at: self.last_activity_at,
            archived_at: self.archived_at,
            highlight: self.highlight,
            focused_task_id: self.focused_task_id,
            parent_id: self.parent_id,
            binding_kind: self.binding_kind,
            binding_data: self.binding_data,
            capabilities: self.capabilities,
            sort_order: self.sort_order,
            final_output: self.final_output,
            merge_status: self.merge_status,
            content: self.content,
            review_json: self.review_json,
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
