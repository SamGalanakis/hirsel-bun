//! Documentation commands
//!
//! Commands for reading project documentation from workspace or active run.

use serde::Serialize;
use std::fs;
use std::path::Path;

use super::ResultExt;
use crate::core::config;
use crate::core::delta::DeltaState;
use crate::core::draft::StartingPoint;
use crate::core::project::ProjectStore;

/// Source of the documentation
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocsSource {
    /// Source type: "workspace" | "run"
    pub kind: String,
    /// Run name (if kind is "run")
    pub run_name: Option<String>,
    /// Run status (if kind is "run")
    pub run_status: Option<String>,
}

/// A documentation file
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocFile {
    /// File name without path (e.g., "architecture.md")
    pub name: String,
    /// Markdown content
    pub content: String,
}

/// Response from get_project_docs
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDocsResponse {
    /// Where the docs came from
    pub source: DocsSource,
    /// List of documentation files
    pub files: Vec<DocFile>,
}

/// Get project documentation files
///
/// Returns docs from the most relevant source:
/// 1. If project has an active run (Working/Eval), read from run_dir/docs/
/// 2. Otherwise, read from workspace/docs_path/
#[tracing::instrument]
#[tauri::command]
pub async fn get_project_docs(
    project_id: i64,
    route_id: i64,
) -> Result<ProjectDocsResponse, String> {
    // Get project to find workspace and docs path
    let store = ProjectStore::open().await.str_err()?;
    let project = store.get_project(project_id).await.str_err()?;

    // Check for active run
    let delta_state = DeltaState::with_route(project_id, route_id);
    let project_run = delta_state.get_project_run().await.str_err()?;

    // Determine source and docs path
    if let Some(ref run) = project_run {
        // Check run status from run's database
        let run_dir = config::run_dir(&run.run_name);
        let run_docs_path = run_dir.join("docs");

        // Check actual run status
        let status = get_run_status(&run_dir);

        // If run has docs directory, use it
        if run_docs_path.exists() && run_docs_path.is_dir() {
            let files = read_docs_from_dir(&run_docs_path)?;
            return Ok(ProjectDocsResponse {
                source: DocsSource {
                    kind: "run".to_string(),
                    run_name: Some(run.run_name.clone()),
                    run_status: Some(status),
                },
                files,
            });
        }
    }

    // Fall back to workspace docs
    let workspace_path = match &project.starting_point {
        StartingPoint::LocalFolder { path } => Some(path.clone()),
        StartingPoint::GitRepo { .. } | StartingPoint::Greenfield => None,
    };

    if let Some(workspace) = workspace_path {
        let docs_dir = Path::new(&workspace).join(&project.docs_path);
        if docs_dir.exists() && docs_dir.is_dir() {
            let files = read_docs_from_dir(&docs_dir)?;
            return Ok(ProjectDocsResponse {
                source: DocsSource {
                    kind: "workspace".to_string(),
                    run_name: None,
                    run_status: None,
                },
                files,
            });
        }
    }

    // No docs found - return empty list
    Ok(ProjectDocsResponse {
        source: DocsSource {
            kind: "workspace".to_string(),
            run_name: None,
            run_status: None,
        },
        files: vec![],
    })
}

/// Read all markdown files from a directory
fn read_docs_from_dir(dir: &Path) -> Result<Vec<DocFile>, String> {
    let mut files = Vec::new();

    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read docs directory: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();

        // Only include markdown files
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "md" {
                    if let Some(name) = path.file_name() {
                        let content = fs::read_to_string(&path)
                            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

                        files.push(DocFile {
                            name: name.to_string_lossy().to_string(),
                            content,
                        });
                    }
                }
            }
        }
    }

    // Sort by name for consistent ordering
    files.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(files)
}

/// Get run status from the run's database
fn get_run_status(run_dir: &Path) -> String {
    use crate::core::Files;

    let files = Files::new(run_dir);

    // Get run_name from the path
    let run_name = match files.run_name() {
        Some(name) => name,
        None => return "unknown".to_string(),
    };

    // We need to use a blocking runtime since this is called from sync context
    // This is a workaround - ideally this function should be async
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle,
        Err(_) => return "unknown".to_string(),
    };

    tokio::task::block_in_place(|| {
        rt.block_on(async {
            use crate::core::state::SQLiteState;
            match SQLiteState::new(&run_name).await {
                Ok(state) => match state.status().await {
                    Ok(status) => format!("{:?}", status).to_lowercase(),
                    Err(_) => "unknown".to_string(),
                },
                Err(_) => "unknown".to_string(),
            }
        })
    })
}
