use std::path::PathBuf;

use crate::backend::draft::create_workspace_provider;
use crate::backend::git::{create_thread_checkout, create_workspace};
use crate::backend::project::{Project, ProjectStore};

pub struct ProjectWorkspace {
    pub workspace_name: String,
    pub workspace_dir: PathBuf,
    pub central_dir: PathBuf,
    pub project: Project,
}

fn slugify(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn workspace_name_for_project(project_id: i64) -> String {
    format!("project-{}", project_id)
}

pub async fn ensure_project_workspace(project_id: i64) -> Result<ProjectWorkspace, String> {
    let store = ProjectStore::open()
        .await
        .map_err(|error| format!("failed to open project store: {}", error))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|error| format!("failed to load project {}: {}", project_id, error))?;

    let workspace_name = workspace_name_for_project(project_id);
    let workspace_dir = crate::backend::config::workspace_dir(&workspace_name);
    let central_dir = workspace_dir.join("work").join("central");

    if central_dir.join(".git").is_dir() {
        return Ok(ProjectWorkspace {
            workspace_name,
            workspace_dir,
            central_dir,
            project,
        });
    }

    if workspace_dir.exists() {
        std::fs::remove_dir_all(&workspace_dir).map_err(|error| {
            format!(
                "failed to remove stale project workspace '{}': {}",
                workspace_dir.display(),
                error
            )
        })?;
    }

    std::fs::create_dir_all(&workspace_dir).map_err(|error| {
        format!(
            "failed to create project workspace '{}': {}",
            workspace_dir.display(),
            error
        )
    })?;

    let workspace_provider = create_workspace_provider();
    let bootstrap = workspace_provider
        .init(&workspace_name, &project.starting_point)
        .await
        .map_err(|error| format!("failed to initialize project workspace: {}", error))?;
    let central_dir = create_workspace(
        &workspace_name,
        &bootstrap.path,
        &crate::backend::config::workspaces_dir(),
    )
    .map_err(|error| format!("failed to create central checkout: {}", error))?;

    Ok(ProjectWorkspace {
        workspace_name,
        workspace_dir,
        central_dir,
        project,
    })
}

pub async fn prepare_thread_checkout(
    project_id: i64,
    title: &str,
) -> Result<(String, String), String> {
    let workspace = ensure_project_workspace(project_id).await?;
    if !workspace.central_dir.join("flake.nix").is_file() {
        return Err(
            "The project central checkout has no flake.nix yet. Ask shepherd to create one before starting threads."
                .to_string(),
        );
    }

    let slug = slugify(title);
    let short = uuid::Uuid::new_v4().simple().to_string();
    let checkout_name = format!(
        "thread-{}-{}",
        if slug.is_empty() {
            "work"
        } else {
            slug.as_str()
        },
        &short[..6]
    );

    let workspace_dir = create_thread_checkout(
        &workspace.workspace_name,
        workspace.central_dir.as_path(),
        &checkout_name,
        Some(&workspace.central_dir),
        &crate::backend::config::workspaces_dir(),
    )
    .map_err(|error| error.to_string())?;

    Ok((workspace_dir.display().to_string(), checkout_name))
}
