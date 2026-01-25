//! Board Service - unified interface for SpecFlow board operations
//!
//! This module provides the BoardService which handles:
//! - Database operations via SpecFlowState
//! - JSON file export/import for AI agents
//! - Local/remote mode routing (like ScribeService)
//!
//! ## Agent Access
//!
//! Agents (like Gyp) read/write JSON files at:
//! `~/.hirsel/projects/{project_id}/board/{island-id}.json`
//!
//! ## Usage
//!
//! ```ignore
//! // Local mode
//! let service = BoardService::new(project_id);
//!
//! // Remote mode
//! let service = BoardService::with_profile(project_id, profile);
//!
//! // Export board to agent files
//! let board_dir = service.export_for_agent().await?;
//!
//! // Import changes from agent files
//! let result = service.import_from_agent().await?;
//! ```

mod types;

pub use types::{AgentEvalView, AgentIslandView, AgentRowView, AgentTaskView, SyncResult};

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;
use tracing::{debug, info, warn};

use crate::core::config::{hirsel_dir, OrchestratorMode, OrchestratorProfile};
use crate::core::http_client::AuthenticatedClient;
use crate::core::specflow::{
    CreateIslandRequest, CreateRowRequest, Island, Row, RowEvalStatus, SpecFlowState, TaskStatus,
    UpdateIslandRequest, UpdateRowRequest,
};

/// Maximum allowed position value (for bounding)
const MAX_POSITION: f64 = 5000.0;

/// Minimum allowed position value
const MIN_POSITION: f64 = 0.0;

/// Error type for board operations
#[derive(Debug, thiserror::Error)]
pub enum BoardError {
    #[error("SpecFlow error: {0}")]
    SpecFlow(#[from] crate::core::specflow::SpecFlowError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] crate::core::http_client::HttpError),
    #[error("Project not found: {0}")]
    ProjectNotFound(i64),
    #[error("Remote error: {0}")]
    Remote(String),
}

pub type BoardResult<T> = Result<T, BoardError>;

/// Service for managing SpecFlow board data with file sync
///
/// Handles both local and remote modes transparently.
pub struct BoardService {
    project_id: i64,
    /// Profile for remote mode (None = local)
    profile: Option<OrchestratorProfile>,
    /// Cached mtime for change detection (local mode only)
    last_sync_times: HashMap<String, SystemTime>,
}

impl BoardService {
    /// Create a new board service for local mode
    pub fn new(project_id: i64) -> Self {
        Self {
            project_id,
            profile: None,
            last_sync_times: HashMap::new(),
        }
    }

    /// Create a board service with a profile (for remote mode)
    pub fn with_profile(project_id: i64, profile: OrchestratorProfile) -> Self {
        Self {
            project_id,
            profile: Some(profile),
            last_sync_times: HashMap::new(),
        }
    }

    /// Check if we should use remote mode
    fn should_use_remote(&self) -> bool {
        self.profile
            .as_ref()
            .map(|p| p.mode == OrchestratorMode::Remote && p.url.is_some())
            .unwrap_or(false)
    }

    /// Get authenticated HTTP client for remote mode
    fn remote_client(&self) -> BoardResult<AuthenticatedClient> {
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| BoardError::Remote("No profile configured".into()))?;

        let url = profile
            .url
            .as_ref()
            .ok_or_else(|| BoardError::Remote("No remote URL configured".into()))?;

        let api_key = profile
            .api_key
            .as_ref()
            .ok_or_else(|| BoardError::Remote("No API key configured".into()))?;

        Ok(AuthenticatedClient::new(url, api_key))
    }

    /// Get the board directory path for this project (local mode)
    pub fn board_dir(&self) -> PathBuf {
        hirsel_dir()
            .join("projects")
            .join(self.project_id.to_string())
            .join("board")
    }

    /// Ensure the board directory exists (local mode)
    fn ensure_board_dir(&self) -> BoardResult<PathBuf> {
        let dir = self.board_dir();
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(dir)
    }

    /// Get path for an island's JSON file (local mode)
    fn island_path(&self, island_id: &str) -> PathBuf {
        self.board_dir().join(format!("{}.json", island_id))
    }

    // ========== PUBLIC API (routes to local or remote) ==========

    /// Export all islands to agent JSON files
    ///
    /// Returns the path to the board directory.
    pub async fn export_for_agent(&self) -> BoardResult<PathBuf> {
        if self.should_use_remote() {
            self.export_remote().await
        } else {
            self.export_local()
        }
    }

    /// Import all agent JSON files to the database
    ///
    /// Returns a summary of changes made.
    pub async fn import_from_agent(&self) -> BoardResult<SyncResult> {
        if self.should_use_remote() {
            self.import_remote().await
        } else {
            self.import_local()
        }
    }

    /// Sync file changes to database (detect changes and import)
    pub async fn sync_file_changes(&mut self) -> BoardResult<SyncResult> {
        if self.should_use_remote() {
            self.import_remote().await
        } else {
            let changed = self.detect_file_changes()?;
            if changed.is_empty() {
                return Ok(SyncResult::default());
            }
            info!("Detected {} changed board files", changed.len());
            self.import_local()
        }
    }

    /// Get the project ID
    pub fn project_id(&self) -> i64 {
        self.project_id
    }

    // ========== LOCAL MODE: EXPORT (DB -> Files) ==========

    fn export_local(&self) -> BoardResult<PathBuf> {
        let board_dir = self.ensure_board_dir()?;
        let state = SpecFlowState::open(self.project_id)?;
        let islands = state.list_islands()?;

        // Track which files should exist
        let mut expected_files: HashSet<String> = HashSet::new();

        for island in &islands {
            let agent_view = self.island_to_agent_view(island);
            let path = self.island_path(&island.id);
            let json = serde_json::to_string_pretty(&agent_view)?;
            std::fs::write(&path, json)?;
            expected_files.insert(format!("{}.json", island.id));
            debug!("Exported island {} to {:?}", island.id, path);
        }

        // Remove stale files (islands that were deleted from DB)
        for entry in std::fs::read_dir(&board_dir)? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.ends_with(".json") && !expected_files.contains(&file_name) {
                std::fs::remove_file(entry.path())?;
                debug!("Removed stale board file: {}", file_name);
            }
        }

        info!("Exported {} islands to {:?}", islands.len(), board_dir);
        Ok(board_dir)
    }

    /// Convert an Island to its agent view
    fn island_to_agent_view(&self, island: &Island) -> AgentIslandView {
        AgentIslandView {
            id: island.id.clone(),
            title: island.name.clone(),
            x: Some(island.x),
            y: Some(island.y),
            rows: island
                .rows
                .iter()
                .map(|r| self.row_to_agent_view(r))
                .collect(),
        }
    }

    /// Convert a Row to its agent view
    fn row_to_agent_view(&self, row: &Row) -> AgentRowView {
        // Convert the row's task to agent task view
        let tasks = if let Some(title) = &row.task_title {
            vec![AgentTaskView {
                id: row.id.clone(),
                subject: title.clone(),
                description: row.task_description.clone(),
                status: row.task_status.as_str().to_string(),
                blocked_by: row.task_blocked_by.clone(),
                subtasks: vec![], // No nested subtasks in current schema
            }]
        } else {
            vec![]
        };

        // Convert eval
        let eval = if row.eval_criterion.is_some() || row.eval_status != RowEvalStatus::Pending {
            Some(AgentEvalView {
                criteria: row.eval_criterion.clone(),
                status: row.eval_status.as_str().to_string(),
            })
        } else {
            None
        };

        AgentRowView {
            id: row.id.clone(),
            spec: row.spec_content.clone(),
            tasks,
            eval,
        }
    }

    // ========== LOCAL MODE: IMPORT (Files -> DB) ==========

    fn import_local(&self) -> BoardResult<SyncResult> {
        let board_dir = self.board_dir();
        if !board_dir.exists() {
            return Ok(SyncResult::default());
        }

        let state = SpecFlowState::open(self.project_id)?;
        let existing_islands = state.list_islands()?;
        let existing_ids: HashSet<String> = existing_islands.iter().map(|i| i.id.clone()).collect();

        let mut result = SyncResult::default();
        let mut seen_ids: HashSet<String> = HashSet::new();

        // Process each JSON file
        for entry in std::fs::read_dir(&board_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            let content = std::fs::read_to_string(&path)?;
            let agent_view: AgentIslandView = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(e) => {
                    warn!("Failed to parse {:?}: {}", path, e);
                    continue;
                }
            };

            seen_ids.insert(agent_view.id.clone());

            if existing_ids.contains(&agent_view.id) {
                // Update existing island
                self.update_island_from_agent(&state, &agent_view)?;
                result.updated.push(agent_view.id.clone());
            } else {
                // Create new island
                self.create_island_from_agent(&state, &agent_view)?;
                result.added.push(agent_view.id.clone());
            }
            result.changes += 1;
        }

        // Delete islands that no longer have files
        for island in &existing_islands {
            if !seen_ids.contains(&island.id) {
                state.delete_island(&island.id)?;
                result.deleted.push(island.id.clone());
                result.changes += 1;
            }
        }

        info!(
            "Imported board: added={}, updated={}, deleted={}",
            result.added.len(),
            result.updated.len(),
            result.deleted.len()
        );

        Ok(result)
    }

    /// Create a new island from agent view
    fn create_island_from_agent(
        &self,
        state: &SpecFlowState,
        view: &AgentIslandView,
    ) -> BoardResult<Island> {
        // Bound position values
        let x = view.x.map(|v| bound_position(v)).unwrap_or(100.0);
        let y = view.y.map(|v| bound_position(v)).unwrap_or(100.0);

        // Create island
        let island = state.create_island(&CreateIslandRequest {
            name: view.title.clone(),
            x,
            y,
            width: 400.0,
        })?;

        // Create rows for this island
        for (position, row_view) in view.rows.iter().enumerate() {
            let row = state.create_row(&CreateRowRequest {
                island_id: island.id.clone(),
                position: Some(position as i32),
            })?;

            // Update row with content
            self.update_row_from_agent_view(state, &row.id, row_view)?;
        }

        // Update the file to use the actual generated ID
        let old_path = self.island_path(&view.id);
        let new_path = self.island_path(&island.id);
        if old_path.exists() && old_path != new_path {
            std::fs::rename(&old_path, &new_path)?;
            debug!("Renamed {:?} to {:?}", old_path, new_path);
        }

        Ok(island)
    }

    /// Update an existing island from agent view
    fn update_island_from_agent(
        &self,
        state: &SpecFlowState,
        view: &AgentIslandView,
    ) -> BoardResult<Island> {
        // Update island properties
        let mut update_req = UpdateIslandRequest {
            name: Some(view.title.clone()),
            ..Default::default()
        };

        if let Some(x) = view.x {
            update_req.x = Some(bound_position(x));
        }
        if let Some(y) = view.y {
            update_req.y = Some(bound_position(y));
        }

        let island = state.update_island(&view.id, &update_req)?;

        // Sync rows
        let existing_rows = &island.rows;
        let existing_row_ids: HashSet<String> =
            existing_rows.iter().map(|r| r.id.clone()).collect();
        let mut seen_row_ids: HashSet<String> = HashSet::new();

        for (position, row_view) in view.rows.iter().enumerate() {
            seen_row_ids.insert(row_view.id.clone());

            if existing_row_ids.contains(&row_view.id) {
                self.update_row_from_agent_view(state, &row_view.id, row_view)?;
            } else {
                let row = state.create_row(&CreateRowRequest {
                    island_id: island.id.clone(),
                    position: Some(position as i32),
                })?;
                self.update_row_from_agent_view(state, &row.id, row_view)?;
            }
        }

        // Delete rows that are no longer in the file
        for row in existing_rows {
            if !seen_row_ids.contains(&row.id) {
                let _ = state.delete_row(&row.id);
            }
        }

        // Reorder rows to match file order
        let row_ids: Vec<String> = view.rows.iter().map(|r| r.id.clone()).collect();
        if !row_ids.is_empty() {
            let _ = state.reorder_rows(&island.id, &row_ids);
        }

        Ok(island)
    }

    /// Update a row from agent view
    fn update_row_from_agent_view(
        &self,
        state: &SpecFlowState,
        row_id: &str,
        view: &AgentRowView,
    ) -> BoardResult<()> {
        let mut update = UpdateRowRequest::default();

        // Spec content
        update.spec_content = view.spec.clone();

        // Task from first task in list
        if let Some(task) = view.tasks.first() {
            update.task_title = Some(task.subject.clone());
            update.task_description = task.description.clone();
            update.task_status = Some(TaskStatus::from_str(&task.status));
            if !task.blocked_by.is_empty() {
                update.task_blocked_by = Some(task.blocked_by.clone());
            }
        }

        // Eval
        if let Some(eval) = &view.eval {
            update.eval_criterion = eval.criteria.clone();
            update.eval_status = Some(RowEvalStatus::from_str(&eval.status));
        }

        state.update_row(row_id, &update)?;
        Ok(())
    }

    // ========== LOCAL MODE: FILE CHANGE DETECTION ==========

    /// Detect which files have changed since last sync
    pub fn detect_file_changes(&mut self) -> BoardResult<Vec<String>> {
        let board_dir = self.board_dir();
        if !board_dir.exists() {
            return Ok(vec![]);
        }

        let mut changed = vec![];

        for entry in std::fs::read_dir(&board_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            let island_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            let mtime = std::fs::metadata(&path)?.modified()?;

            if let Some(last_mtime) = self.last_sync_times.get(&island_id) {
                if mtime > *last_mtime {
                    changed.push(island_id.clone());
                }
            } else {
                changed.push(island_id.clone());
            }

            self.last_sync_times.insert(island_id, mtime);
        }

        Ok(changed)
    }

    // ========== REMOTE MODE ==========

    async fn export_remote(&self) -> BoardResult<PathBuf> {
        let client = self.remote_client()?;
        let path: String = client
            .post(
                &format!("/api/board/{}/export", self.project_id),
                &serde_json::json!({}),
            )
            .await?;
        Ok(PathBuf::from(path))
    }

    async fn import_remote(&self) -> BoardResult<SyncResult> {
        let client = self.remote_client()?;
        let result: SyncResult = client
            .post(
                &format!("/api/board/{}/import", self.project_id),
                &serde_json::json!({}),
            )
            .await?;
        Ok(result)
    }

    // ========== SYNC VARIANTS (for server routes) ==========

    /// Export to files synchronously (for use in blocking contexts)
    pub fn export_local_sync(&self) -> BoardResult<PathBuf> {
        self.export_local()
    }

    /// Import from files synchronously (for use in blocking contexts)
    pub fn import_local_sync(&self) -> BoardResult<SyncResult> {
        self.import_local()
    }
}

/// Bound a position value to the allowed range
fn bound_position(value: f64) -> f64 {
    value.clamp(MIN_POSITION, MAX_POSITION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bound_position() {
        assert_eq!(bound_position(100.0), 100.0);
        assert_eq!(bound_position(-100.0), 0.0);
        assert_eq!(bound_position(10000.0), 5000.0);
    }

    #[test]
    fn test_agent_view_serialization() {
        let view = AgentIslandView {
            id: "test-id".to_string(),
            title: "Test Island".to_string(),
            x: Some(100.0),
            y: Some(200.0),
            rows: vec![],
        };

        let json = serde_json::to_string_pretty(&view).unwrap();
        assert!(json.contains("\"title\": \"Test Island\""));

        let parsed: AgentIslandView = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test-id");
        assert_eq!(parsed.title, "Test Island");
    }
}
