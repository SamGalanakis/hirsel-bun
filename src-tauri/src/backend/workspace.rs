use std::path::{Path, PathBuf};

use git2::Repository;

use crate::backend::draft::create_workspace_provider;
use crate::backend::git::{create_thread_checkout, create_workspace, get_current_branch};
use crate::backend::project::{Project, ProjectStore};
use crate::backend::shepherd_threads::ShepherdThreadStore;

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

pub fn legacy_workspace_name_for_project_id(project_id: i64) -> String {
    format!("project-{}", project_id)
}

pub fn workspace_name_for_project(project: &Project) -> String {
    format!("project-{}-{}", project.id, project.workspace_key)
}

fn thread_checkout_name(title: &str) -> String {
    let slug = slugify(title);
    let short = uuid::Uuid::new_v4().simple().to_string();
    format!(
        "thread-{}-{}",
        if slug.is_empty() {
            "work"
        } else {
            slug.as_str()
        },
        &short[..6]
    )
}

fn repo_has_local_changes(repo_dir: &Path) -> Result<bool, String> {
    let repo = Repository::open(repo_dir).map_err(|error| {
        format!(
            "failed to open thread checkout '{}': {}",
            repo_dir.display(),
            error
        )
    })?;
    let statuses = repo.statuses(None).map_err(|error| {
        format!(
            "failed to read git status for '{}': {}",
            repo_dir.display(),
            error
        )
    })?;
    Ok(!statuses.is_empty())
}

fn remove_checkout_path(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|error| {
            format!(
                "failed to remove stale thread checkout '{}': {}",
                path.display(),
                error
            )
        })
    } else {
        std::fs::remove_file(path).map_err(|error| {
            format!(
                "failed to remove stale thread checkout '{}': {}",
                path.display(),
                error
            )
        })
    }
}

fn thread_checkout_needs_rebuild(
    checkout_dir: &Path,
    checkout_name: &str,
    central_dir: &Path,
) -> Result<bool, String> {
    if !checkout_dir.join(".git").is_dir() {
        return Ok(true);
    }

    let current_branch = match get_current_branch(checkout_dir) {
        Ok(branch) => branch,
        Err(_) => return Ok(true),
    };
    if current_branch == "central" && checkout_name != "central" {
        return Ok(true);
    }

    if central_dir.join("flake.nix").is_file()
        && !checkout_dir.join("flake.nix").is_file()
        && !repo_has_local_changes(checkout_dir)?
    {
        return Ok(true);
    }

    Ok(false)
}

pub async fn ensure_project_workspace(project_id: i64) -> Result<ProjectWorkspace, String> {
    let store = ProjectStore::open()
        .await
        .map_err(|error| format!("failed to open project store: {}", error))?;
    let project = store
        .get_project(project_id)
        .await
        .map_err(|error| format!("failed to load project {}: {}", project_id, error))?;

    let workspace_name = workspace_name_for_project(&project);
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

    let checkout_name = thread_checkout_name(title);

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

pub async fn ensure_thread_checkout(
    project_id: i64,
    thread_id: &str,
) -> Result<(String, String), String> {
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|error| format!("failed to open shepherd thread store: {}", error))?;
    let thread = store
        .get_thread(thread_id)
        .await
        .map_err(|error| format!("failed to load thread {}: {}", thread_id, error))?;
    if thread.project_id != project_id {
        return Err(format!(
            "thread {} does not belong to project {}",
            thread_id, project_id
        ));
    }

    let workspace = ensure_project_workspace(project_id).await?;
    if !workspace.central_dir.join("flake.nix").is_file() {
        return Err(
            "The project central checkout has no flake.nix yet. Ask shepherd to create one before starting threads."
                .to_string(),
        );
    }

    let checkout_name = thread
        .checkout_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| thread_checkout_name(&thread.title));
    let checkout_dir = workspace.workspace_dir.join("work").join(&checkout_name);

    let rebuild = if !checkout_dir.exists() {
        true
    } else {
        thread_checkout_needs_rebuild(&checkout_dir, &checkout_name, &workspace.central_dir)?
    };

    if rebuild {
        remove_checkout_path(&checkout_dir)?;
        create_thread_checkout(
            &workspace.workspace_name,
            workspace.central_dir.as_path(),
            &checkout_name,
            Some(&workspace.central_dir),
            &crate::backend::config::workspaces_dir(),
        )
        .map_err(|error| {
            format!(
                "failed to materialize thread checkout '{}': {}",
                checkout_name, error
            )
        })?;
    }

    let workspace_path = checkout_dir.display().to_string();
    if thread.workspace_path.as_deref() != Some(workspace_path.as_str())
        || thread.checkout_name.as_deref() != Some(checkout_name.as_str())
    {
        store
            .update_thread(
                &thread.id,
                None,
                None,
                None,
                None,
                Some(Some(&workspace_path)),
                Some(Some(&checkout_name)),
            )
            .await
            .map_err(|error| {
                format!(
                    "failed to update thread checkout '{}': {}",
                    thread.id, error
                )
            })?;
    }

    Ok((workspace_path, checkout_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::app;
    use crate::backend::config::testing::TestEnv;
    use crate::backend::draft::StartingPoint;
    use crate::backend::shepherd_threads::ShepherdThreadStore;
    use git2::Signature;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    const TEST_FLAKE: &str = r#"
{
  description = "hirsel thread repair smoke";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = builtins.currentSystem;
      pkgs = import nixpkgs { inherit system; };
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [ bash git ];
      };
    };
}
"#;

    fn create_local_source_repo() -> TempDir {
        let dir = TempDir::new().expect("create temp repo");
        let repo = Repository::init(dir.path()).expect("init repo");
        let readme = dir.path().join("README.md");
        let mut file = File::create(&readme).expect("create readme");
        writeln!(file, "# smoke").expect("write readme");

        let mut index = repo.index().expect("index");
        index
            .add_path(Path::new("README.md"))
            .expect("add readme to index");
        index.write().expect("write index");
        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        let sig = Signature::now("hirsel", "hirsel@test").expect("signature");
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .expect("initial commit");
        dir
    }

    #[tokio::test]
    async fn ensure_thread_checkout_repairs_legacy_central_branch_snapshot() {
        let _env = TestEnv::builder().build();
        let source = create_local_source_repo();
        let project = app::create_project(
            "smoke".to_string(),
            StartingPoint::LocalFolder {
                path: source.path().display().to_string(),
            },
            None,
            None,
            None,
        )
        .await
        .expect("create project");

        let workspace = ensure_project_workspace(project.id)
            .await
            .expect("materialize central workspace");
        std::fs::write(workspace.central_dir.join("flake.nix"), TEST_FLAKE).expect("write flake");

        let (workspace_path, checkout_name) = prepare_thread_checkout(project.id, "Smoke thread")
            .await
            .expect("create thread checkout");
        let store = ShepherdThreadStore::open()
            .await
            .expect("open thread store");
        let thread = store
            .create_thread(
                project.id,
                "Smoke thread",
                "Read only smoke task",
                "summary",
                Some(&workspace_path),
                Some(&checkout_name),
            )
            .await
            .expect("create thread row");

        let checkout_dir = PathBuf::from(&workspace_path);
        std::fs::remove_file(checkout_dir.join("flake.nix")).expect("remove thread flake");
        let repo = Repository::open(&checkout_dir).expect("open thread repo");
        repo.set_head("refs/heads/central")
            .expect("switch to legacy central branch");

        let (repaired_path, repaired_checkout_name) =
            ensure_thread_checkout(project.id, &thread.id)
                .await
                .expect("repair thread checkout");

        assert_eq!(repaired_checkout_name, checkout_name);
        assert_eq!(repaired_path, workspace_path);
        assert!(
            checkout_dir.join("flake.nix").is_file(),
            "repair should restore the central flake snapshot"
        );
        assert_eq!(
            get_current_branch(&checkout_dir).expect("current branch"),
            checkout_name
        );
    }
}
