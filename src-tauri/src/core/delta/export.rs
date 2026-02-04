//! Delta tree export/import for Gyp agent access
//!
//! Content files live at `routes/{route_name}/board/tasks/{id}.md` for direct editing.
//! Structure is managed via MCP tools (board_view, board_task, etc.)
//!
//! File structure:
//! ```
//! ~/.hirsel/projects/{project_id}/routes/{route_name}/board/
//! └── tasks/
//!     ├── build-api.md     # Task content
//!     ├── api-test.md      # Eval content
//!     └── ...
//! ```
//!
//! NOTE: No board.json - structure is in database, accessed via MCP tools.

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::state::DeltaState;
use super::types::UpdateDraftNodeRequest;
use crate::core::route::{RouteFiles, RouteStore};

/// Block on an async future in a sync context.
/// If already running in an async context, uses the current runtime.
fn block_on<F: Future>(f: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(f)),
        Err(_) => tokio::runtime::Runtime::new()
            .expect("Failed to create tokio runtime")
            .block_on(f),
    }
}

/// Error type for export operations
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("State error: {0}")]
    State(#[from] super::state::DeltaStateError),
}

pub type ExportResult<T> = Result<T, ExportError>;

/// Result of a sync operation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncResult {
    /// Number of changes applied
    pub changes: usize,
    /// Nodes that were added
    pub nodes_added: Vec<String>,
    /// Nodes that were updated
    pub nodes_updated: Vec<String>,
    /// Nodes that were deleted
    pub nodes_deleted: Vec<String>,
}

/// Exporter for content files
pub struct DeltaExporter {
    project_id: i64,
    #[allow(dead_code)]
    route_id: i64,
    route_name: String,
    state: DeltaState,
}

impl DeltaExporter {
    /// Create a new exporter for a project route
    pub fn new(project_id: i64, route_id: i64) -> Self {
        // Look up route name
        let route_name = if route_id == 0 {
            "main".to_string()
        } else {
            block_on(async {
                let store = RouteStore::new(project_id).await.ok()?;
                let route = store.get_route(route_id).await.ok()?;
                Some(route.name)
            })
            .unwrap_or_else(|| "main".to_string())
        };

        Self {
            project_id,
            route_id,
            route_name,
            state: DeltaState::with_route(project_id, route_id),
        }
    }

    /// Get the board directory path for this route
    pub fn board_dir(&self) -> PathBuf {
        RouteFiles::new(self.project_id, &self.route_name).board_dir()
    }

    /// Ensure the board directory exists
    fn ensure_board_dir(&self) -> ExportResult<PathBuf> {
        let dir = self.board_dir();
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(dir)
    }

    // =========================================================================
    // Export
    // =========================================================================

    /// Export content files to tasks/{id}.md
    ///
    /// Structure is NOT exported - it's managed via MCP tools.
    /// Only content files are written for agent editing.
    pub fn export_for_agent(&mut self) -> ExportResult<PathBuf> {
        let board_dir = self.ensure_board_dir()?;
        let tasks_dir = board_dir.join("tasks");
        if !tasks_dir.exists() {
            std::fs::create_dir_all(&tasks_dir)?;
        }

        // Get all draft nodes for content export
        let all_nodes = block_on(self.state.get_draft_nodes())?;

        // Export content files to tasks/ directory
        let mut exported_ids = HashSet::new();
        for node in &all_nodes {
            let content_path = tasks_dir.join(format!("{}.md", node.id));
            // Only write if file doesn't exist or content differs
            let should_write = match std::fs::read_to_string(&content_path) {
                Ok(existing) => existing != node.content,
                Err(_) => true,
            };
            if should_write {
                std::fs::write(&content_path, &node.content)?;
                debug!("Exported content file: {:?}", content_path);
            }
            exported_ids.insert(node.id.clone());
        }

        // Clean up stale content files (nodes that no longer exist)
        if let Ok(entries) = std::fs::read_dir(&tasks_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "md").unwrap_or(false) {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if !exported_ids.contains(stem) {
                            std::fs::remove_file(&path)?;
                            debug!("Removed stale content file: {:?}", path);
                        }
                    }
                }
            }
        }

        // Clean up old board.json if it exists (no longer used)
        let board_json = board_dir.join("board.json");
        if board_json.exists() {
            std::fs::remove_file(&board_json)?;
            debug!("Removed legacy board.json");
        }

        info!(
            "Exported {} content files to {:?}",
            exported_ids.len(),
            tasks_dir
        );
        Ok(board_dir)
    }

    // =========================================================================
    // Import (Content Only)
    // =========================================================================

    /// Sync content changes from tasks/*.md files back to database
    ///
    /// NOTE: Structure is managed via MCP tools - this only syncs content.
    /// Files that don't match existing nodes are ignored.
    pub fn sync_file_changes(&mut self) -> ExportResult<SyncResult> {
        let tasks_dir = self.board_dir().join("tasks");
        if !tasks_dir.exists() {
            return Ok(SyncResult::default());
        }

        let mut result = SyncResult::default();

        // Read all .md files and sync content to matching nodes
        if let Ok(entries) = std::fs::read_dir(&tasks_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "md").unwrap_or(false) {
                    if let Some(node_id) = path.file_stem().and_then(|s| s.to_str()) {
                        // Try to find matching node in database
                        if let Ok(existing) = block_on(self.state.get_draft_node(node_id)) {
                            // Read file content
                            if let Ok(file_content) = std::fs::read_to_string(&path) {
                                // Update if content differs
                                if existing.content != file_content {
                                    if block_on(self.state.update_draft_node(
                                        node_id,
                                        &UpdateDraftNodeRequest {
                                            content: Some(file_content),
                                            ..Default::default()
                                        },
                                    ))
                                    .is_ok()
                                    {
                                        result.nodes_updated.push(node_id.to_string());
                                    }
                                }
                            }
                        }
                        // Files without matching nodes are ignored - structure comes from MCP
                    }
                }
            }
        }

        result.changes = result.nodes_updated.len();

        if result.changes > 0 {
            debug!("Synced {} content files", result.changes);
        }

        Ok(result)
    }
}
