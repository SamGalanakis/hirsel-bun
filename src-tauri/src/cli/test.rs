//! Implementation of the `hirsel test` command - run e2e test scenarios.
//!
//! This command runs predefined test scenarios from the tests/scenarios directory.
//! Each scenario contains a spec.md, eval.md, and a project folder to test against.

use std::fs;
use std::path::PathBuf;

use crate::cli::go::{run as run_go, GoError};
use crate::cli::GoArgs;

// =============================================================================
// Error Types
// =============================================================================

#[derive(Debug)]
pub enum TestError {
    ScenarioNotFound(String),
    InvalidScenario(String),
    Go(GoError),
    Io(std::io::Error),
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::ScenarioNotFound(name) => write!(f, "Scenario not found: {}", name),
            TestError::InvalidScenario(msg) => write!(f, "Invalid scenario: {}", msg),
            TestError::Go(e) => write!(f, "Go error: {}", e),
            TestError::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for TestError {}

impl From<GoError> for TestError {
    fn from(e: GoError) -> Self {
        TestError::Go(e)
    }
}

impl From<std::io::Error> for TestError {
    fn from(e: std::io::Error) -> Self {
        TestError::Io(e)
    }
}

pub type TestResult<T> = Result<T, TestError>;

// =============================================================================
// Scenario Discovery
// =============================================================================

/// Get the scenarios directory path
pub fn get_scenarios_dir() -> PathBuf {
    // Look for scenarios in common locations
    let possible_paths = [
        // Development: relative to crate root
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|p| p.join("tests/scenarios"))
            .unwrap_or_default(),
        // Installed: relative to binary
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("../share/hirsel/scenarios")))
            .unwrap_or_default(),
        // Current directory
        PathBuf::from("tests/scenarios"),
    ];

    for path in possible_paths {
        if path.exists() {
            return path;
        }
    }

    // Default to the first option even if it doesn't exist
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("tests/scenarios"))
        .unwrap_or_else(|| PathBuf::from("tests/scenarios"))
}

/// List available test scenarios
pub fn list_scenarios() -> TestResult<Vec<ScenarioInfo>> {
    let scenarios_dir = get_scenarios_dir();

    if !scenarios_dir.exists() {
        return Ok(vec![]);
    }

    let mut scenarios = Vec::new();

    for entry in fs::read_dir(&scenarios_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let spec_path = path.join("spec.md");
            let eval_path = path.join("eval.md");
            let project_path = path.join("project");

            if spec_path.exists() {
                let has_eval = eval_path.exists();
                let has_project = project_path.exists();

                // Read first line of spec as description
                let description = fs::read_to_string(&spec_path)
                    .ok()
                    .and_then(|content| {
                        content
                            .lines()
                            .find(|l| l.starts_with("# "))
                            .map(|l| l.trim_start_matches("# ").to_string())
                    })
                    .unwrap_or_default();

                scenarios.push(ScenarioInfo {
                    name,
                    description,
                    has_eval,
                    has_project,
                    path,
                });
            }
        }
    }

    scenarios.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(scenarios)
}

/// Information about a test scenario
#[derive(Debug, Clone)]
pub struct ScenarioInfo {
    pub name: String,
    pub description: String,
    pub has_eval: bool,
    pub has_project: bool,
    pub path: PathBuf,
}

// =============================================================================
// Run Scenario
// =============================================================================

/// Run a test scenario
pub fn run_scenario(
    scenario_name: &str,
    run_name: Option<&str>,
    workers: Option<&str>,
    yolo: bool,
) -> TestResult<String> {
    let scenarios_dir = get_scenarios_dir();
    let scenario_path = scenarios_dir.join(scenario_name);

    if !scenario_path.exists() {
        return Err(TestError::ScenarioNotFound(scenario_name.to_string()));
    }

    let spec_path = scenario_path.join("spec.md");
    let eval_path = scenario_path.join("eval.md");
    let project_path = scenario_path.join("project");

    if !spec_path.exists() {
        return Err(TestError::InvalidScenario(format!(
            "Missing spec.md in scenario '{}'",
            scenario_name
        )));
    }

    // Determine run name
    let actual_run_name = run_name
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("test-{}", scenario_name));

    // Build GoArgs
    let args = GoArgs {
        run_name: actual_run_name.clone(),
        spec: spec_path.to_str().unwrap_or("").to_string(),
        workers: workers.unwrap_or("1").to_string(),
        time_limit: None,
        remote: None,
        sandbox: false,
        yolo,
        eval: if eval_path.exists() {
            Some(eval_path.to_str().unwrap_or("").to_string())
        } else {
            None
        },
        template: None,
        project: if project_path.exists() {
            Some(project_path.to_str().unwrap_or("").to_string())
        } else {
            None
        },
        max_iterations: None,
        pause_mode: None,
        draft: false,
        assets: None,
    };

    // Run the scenario
    let output = run_go(&args)?;

    Ok(output.run_name)
}

// =============================================================================
// CLI Execution
// =============================================================================

/// Execute the test command
pub fn execute(
    scenario: Option<&str>,
    run_name: Option<&str>,
    workers: Option<&str>,
    yolo: bool,
    json: bool,
) -> TestResult<()> {
    match scenario {
        Some(name) => {
            // Run specific scenario
            let run_name = run_scenario(name, run_name, workers, yolo)?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "started",
                        "scenario": name,
                        "run_name": run_name,
                    })
                );
            } else {
                println!("Started run: {}", run_name);
            }
        }
        None => {
            // List scenarios
            let scenarios = list_scenarios()?;

            if json {
                let json_output: Vec<_> = scenarios
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "name": s.name,
                            "description": s.description,
                            "has_eval": s.has_eval,
                            "has_project": s.has_project,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&json_output).unwrap());
            } else {
                if scenarios.is_empty() {
                    println!("No test scenarios found.");
                    println!("Scenarios should be in: {}", get_scenarios_dir().display());
                } else {
                    println!("Available test scenarios:\n");
                    for s in &scenarios {
                        let indicators = format!(
                            "{}{}",
                            if s.has_eval { "E" } else { " " },
                            if s.has_project { "P" } else { " " }
                        );
                        println!("  [{}] {}", indicators, s.name);
                        if !s.description.is_empty() {
                            println!("       {}", s.description);
                        }
                    }
                    println!();
                    println!("Legend: E=has eval, P=has project");
                    println!();
                    println!("Run a scenario with: hirsel test <scenario-name>");
                }
            }
        }
    }

    Ok(())
}
