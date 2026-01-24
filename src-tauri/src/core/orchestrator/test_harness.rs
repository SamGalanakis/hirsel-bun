//! Test harness for orchestrator testing
//!
//! Provides in-memory testing capabilities for runs without spawning real processes.
//!
//! Note: Tests must run serially because the orchestrator uses global functions
//! (config::run_dir, config::list_runs) that read from HIRSEL_ROOT env var.

use std::path::PathBuf;
use std::sync::Mutex;

use super::{CreateRunRequest, Orchestrator, OrchestratorResult, StartingPoint};
use crate::core::config::Config;
use crate::core::state::{SQLiteState, Status, WorkerStatus, WorkerUpdate};

/// Global mutex to serialize tests that modify HIRSEL_ROOT env var.
/// This prevents tests from interfering with each other.
static TEST_MUTEX: Mutex<()> = Mutex::new(());

/// Test harness for orchestrator integration tests.
///
/// Provides a clean environment with temporary directories for testing
/// orchestrator operations without spawning real worker processes.
///
/// **Important**: This harness modifies the HIRSEL_ROOT env var. Tests using this
/// harness are serialized via a mutex to prevent interference.
pub struct TestHarness {
    pub orchestrator: super::LocalOrchestrator,
    pub config: Config,
    pub temp_dir: tempfile::TempDir,
    /// Guard to keep the mutex locked while harness is in use
    _guard: std::sync::MutexGuard<'static, ()>,
    /// Previous HIRSEL_ROOT value to restore on drop
    prev_hirsel_root: Option<String>,
}

impl TestHarness {
    /// Create a new test harness with temporary directories.
    ///
    /// This acquires a mutex to ensure only one test runs at a time,
    /// and sets HIRSEL_ROOT to a temporary directory.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Acquire mutex to serialize tests (ignore poisoning from previous test failures)
        let guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        // Save previous HIRSEL_ROOT value
        let prev_hirsel_root = std::env::var("HIRSEL_ROOT").ok();

        let temp_dir = tempfile::TempDir::new()?;

        // Create directories
        let runs_dir = temp_dir.path().join("runs");
        std::fs::create_dir_all(&runs_dir)?;

        // Set HIRSEL_ROOT for global functions (config::run_dir, config::list_runs)
        std::env::set_var("HIRSEL_ROOT", temp_dir.path().to_str().unwrap());

        // Create a config with the temp directory as root
        let config = Config {
            root: temp_dir.path().to_path_buf(),
            ..Config::default()
        };

        let orchestrator = super::LocalOrchestrator::new(config.clone());

        Ok(Self {
            orchestrator,
            config,
            temp_dir,
            _guard: guard,
            prev_hirsel_root,
        })
    }

    /// Get the runs directory path.
    pub fn runs_dir(&self) -> PathBuf {
        self.temp_dir.path().join("runs")
    }

    /// Create a run with the given name and spec (draft mode - no workers spawned).
    pub async fn create_draft_run(
        &self,
        name: &str,
        spec: &str,
    ) -> OrchestratorResult<super::CreateRunResponse> {
        let request = CreateRunRequest {
            name: name.to_string(),
            spec: spec.to_string(),
            starting_point: None,
            runner: None,
            worker_scale: Some(1),
            time_limit_minutes: None,
            max_iterations: None,
            human_in_the_loop: Some(false),
            eval: None,
            tailscale_oauth: None,
        };

        self.orchestrator.create_run(request).await
    }

    /// Create a run with a mock project directory (draft mode).
    pub async fn create_run_with_project(
        &self,
        name: &str,
        spec: &str,
    ) -> OrchestratorResult<super::CreateRunResponse> {
        // Create a mock project directory
        let project_dir = self.temp_dir.path().join("project");
        std::fs::create_dir_all(&project_dir)
            .map_err(|e| super::OrchestratorError::Other(e.to_string()))?;

        // Initialize as git repo
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&project_dir)
            .output()
            .map_err(|e| super::OrchestratorError::Other(e.to_string()))?;

        // Create a file
        std::fs::write(project_dir.join("README.md"), "# Test Project")
            .map_err(|e| super::OrchestratorError::Other(e.to_string()))?;

        // Add and commit
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&project_dir)
            .output()
            .ok();
        std::process::Command::new("git")
            .args(["commit", "-m", "Initial"])
            .current_dir(&project_dir)
            .output()
            .ok();

        let request = CreateRunRequest {
            name: name.to_string(),
            spec: spec.to_string(),
            starting_point: Some(StartingPoint::LocalFolder {
                path: project_dir.to_string_lossy().to_string(),
            }),
            runner: None,
            worker_scale: Some(1),
            time_limit_minutes: None,
            max_iterations: None,
            human_in_the_loop: Some(false),
            eval: None,
            tailscale_oauth: None,
        };

        self.orchestrator.create_run(request).await
    }

    /// Get SQLite state for a run.
    pub fn get_state(&self, run_name: &str) -> Result<SQLiteState, Box<dyn std::error::Error>> {
        let db_path = self.runs_dir().join(run_name).join("hirsel.db");
        SQLiteState::new(db_path).map_err(|e| e.into())
    }

    /// Simulate a worker reporting ready (status = Awaiting).
    pub fn simulate_worker_ready(
        &self,
        run_name: &str,
        worker_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let state = self.get_state(run_name)?;
        state.update_worker(
            worker_name,
            WorkerUpdate {
                status: Some(WorkerStatus::Awaiting),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    /// Simulate a worker completing (status = Paused, task done).
    pub fn simulate_worker_done(
        &self,
        run_name: &str,
        worker_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let state = self.get_state(run_name)?;

        // Find claimed task and complete it
        let tasks = state.get_tasks()?;
        for task in tasks {
            if task.claimed_by.as_deref() == Some(worker_name) {
                state.complete_task(&task.id, worker_name)?;
                break;
            }
        }

        // Set worker to paused
        state.update_worker(
            worker_name,
            WorkerUpdate {
                status: Some(WorkerStatus::Paused),
                ..Default::default()
            },
        )?;

        Ok(())
    }

    /// Simulate a worker error.
    pub fn simulate_worker_error(
        &self,
        run_name: &str,
        worker_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let state = self.get_state(run_name)?;
        state.update_worker(
            worker_name,
            WorkerUpdate {
                status: Some(WorkerStatus::Error),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    /// Assert the run is in the expected status.
    pub fn assert_run_status(
        &self,
        run_name: &str,
        expected: Status,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let state = self.get_state(run_name)?;
        let actual = state.status()?;
        if actual != expected {
            return Err(format!("Expected run status {:?}, got {:?}", expected, actual).into());
        }
        Ok(())
    }

    /// Assert a worker is in the expected status.
    pub fn assert_worker_status(
        &self,
        run_name: &str,
        worker_name: &str,
        expected: WorkerStatus,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let state = self.get_state(run_name)?;
        let worker = state
            .get_worker(worker_name)?
            .ok_or_else(|| format!("Worker {} not found", worker_name))?;
        if worker.status != expected {
            return Err(format!(
                "Expected worker {} status {:?}, got {:?}",
                worker_name, expected, worker.status
            )
            .into());
        }
        Ok(())
    }

    /// Assert a task exists with the expected status.
    pub fn assert_task_status(
        &self,
        run_name: &str,
        task_id: &str,
        expected: crate::core::state::TaskStatus,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let state = self.get_state(run_name)?;
        let task = state
            .get_task(task_id)?
            .ok_or_else(|| format!("Task {} not found", task_id))?;
        if task.status != expected {
            return Err(format!(
                "Expected task {} status {:?}, got {:?}",
                task_id, expected, task.status
            )
            .into());
        }
        Ok(())
    }

    /// Get worker names for a run.
    pub fn get_worker_names(
        &self,
        run_name: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let state = self.get_state(run_name)?;
        let workers = state.get_workers()?;
        Ok(workers.into_iter().map(|w| w.name).collect())
    }

    /// Get task IDs for a run.
    pub fn get_task_ids(&self, run_name: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let state = self.get_state(run_name)?;
        let tasks = state.get_tasks()?;
        Ok(tasks.into_iter().map(|t| t.id).collect())
    }
}

impl Default for TestHarness {
    fn default() -> Self {
        Self::new().expect("Failed to create test harness")
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        // Restore previous HIRSEL_ROOT value
        match &self.prev_hirsel_root {
            Some(value) => std::env::set_var("HIRSEL_ROOT", value),
            None => std::env::remove_var("HIRSEL_ROOT"),
        }
        // Mutex guard releases automatically
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_draft_run() {
        let harness = TestHarness::new().unwrap();
        let result = harness
            .create_draft_run("test-run", "Build a hello world")
            .await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.name, "test-run");

        // Verify run exists
        harness
            .assert_run_status("test-run", Status::Draft)
            .unwrap();
    }

    #[tokio::test]
    async fn test_worker_simulation() {
        let harness = TestHarness::new().unwrap();
        harness
            .create_draft_run("test-run", "Build something")
            .await
            .unwrap();

        // Get the worker name (auto-generated)
        let workers = harness.get_worker_names("test-run").unwrap();
        assert!(!workers.is_empty());
        let worker_name = &workers[0];

        // Simulate worker ready
        harness
            .simulate_worker_ready("test-run", worker_name)
            .unwrap();
        harness
            .assert_worker_status("test-run", worker_name, WorkerStatus::Awaiting)
            .unwrap();

        // Simulate worker done
        harness
            .simulate_worker_done("test-run", worker_name)
            .unwrap();
        harness
            .assert_worker_status("test-run", worker_name, WorkerStatus::Paused)
            .unwrap();
    }

    #[tokio::test]
    async fn test_task_operations() {
        let harness = TestHarness::new().unwrap();
        harness
            .create_draft_run("test-run", "Build something")
            .await
            .unwrap();

        // Verify initial scope task exists
        let tasks = harness.get_task_ids("test-run").unwrap();
        assert!(tasks.contains(&"scope".to_string()));

        // Add a new task
        let state = harness.get_state("test-run").unwrap();
        state.add_task("task-1", "First task", None, None).unwrap();

        harness
            .assert_task_status("test-run", "task-1", crate::core::state::TaskStatus::Todo)
            .unwrap();

        // Claim and complete the task
        state.claim_task("task-1", "test-worker").unwrap();
        state.complete_task("task-1", "test-worker").unwrap();
        harness
            .assert_task_status("test-run", "task-1", crate::core::state::TaskStatus::Done)
            .unwrap();
    }

    #[tokio::test]
    async fn test_list_runs() {
        let harness = TestHarness::new().unwrap();

        // Create multiple runs
        harness
            .create_draft_run("run-1", "First run")
            .await
            .unwrap();
        harness
            .create_draft_run("run-2", "Second run")
            .await
            .unwrap();

        // List runs
        let runs = harness.orchestrator.list_runs().await.unwrap();
        assert_eq!(runs.len(), 2);

        let names: Vec<_> = runs.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"run-1"));
        assert!(names.contains(&"run-2"));
    }
}
