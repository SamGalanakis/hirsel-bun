//! Implementation of the `hirsel reset` command.
//!
//! Provides options to reset runs, config, or everything.
//! Requires explicit confirmation by typing "reset".

use crate::core::config;
use std::fs;
use std::io::{self, Write};

/// What to reset
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetTarget {
    Runs,
    Config,
    All,
}

/// Result of the reset operation
#[derive(Debug)]
pub struct ResetResult {
    pub runs_deleted: usize,
    pub config_reset: bool,
}

/// Show what would be reset and get confirmation
pub fn run_reset(target: ResetTarget, json: bool) -> anyhow::Result<()> {
    // Gather info about what would be deleted
    let runs_dir = config::runs_dir();
    let config_path = config::hirsel_dir().join("config.toml");

    let runs_to_delete: Vec<String> = if runs_dir.exists() {
        fs::read_dir(&runs_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .collect()
    } else {
        vec![]
    };

    let will_delete_runs = matches!(target, ResetTarget::Runs | ResetTarget::All);
    let will_reset_config = matches!(target, ResetTarget::Config | ResetTarget::All);

    // Show what will be affected
    if json {
        let empty_vec: Vec<String> = vec![];
        let preview = serde_json::json!({
            "target": match target {
                ResetTarget::Runs => "runs",
                ResetTarget::Config => "config",
                ResetTarget::All => "all",
            },
            "runs_to_delete": if will_delete_runs { &runs_to_delete } else { &empty_vec },
            "runs_count": if will_delete_runs { runs_to_delete.len() } else { 0 },
            "config_path": if will_reset_config { config_path.to_string_lossy().to_string() } else { String::new() },
            "will_reset_config": will_reset_config,
            "requires_confirmation": true,
        });
        println!("{}", serde_json::to_string_pretty(&preview)?);
        return Ok(());
    }

    // Interactive mode - show preview
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                    ⚠️  RESET PREVIEW ⚠️                        ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    if will_delete_runs {
        if runs_to_delete.is_empty() {
            println!("📁 Runs: (none found)");
        } else {
            println!("📁 Runs to delete ({}):", runs_to_delete.len());
            for run in &runs_to_delete {
                println!("   • {}", run);
            }
        }
        println!("   Directory: {}", runs_dir.display());
        println!();
    }

    if will_reset_config {
        println!("⚙️  Config will be reset to defaults:");
        println!("   Path: {}", config_path.display());
        println!();
    }

    if !will_delete_runs && !will_reset_config {
        println!("Nothing to reset.");
        return Ok(());
    }

    // Require typing "reset" to confirm
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  This action is IRREVERSIBLE. All data will be deleted.     ║");
    println!("║  Type 'reset' to confirm:                                   ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    print!("> ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.trim() != "reset" {
        println!("\nReset cancelled.");
        return Ok(());
    }

    // Perform the reset
    let result = execute_reset(target, &runs_dir, &config_path, runs_to_delete.len())?;

    println!();
    println!("✓ Reset complete:");
    if result.runs_deleted > 0 {
        println!("  • Deleted {} run(s)", result.runs_deleted);
    }
    if result.config_reset {
        println!("  • Config reset to defaults");
    }

    Ok(())
}

/// Execute the reset with confirmation already obtained (for programmatic use)
pub fn execute_reset_confirmed(target: ResetTarget, json: bool) -> anyhow::Result<ResetResult> {
    let runs_dir = config::runs_dir();
    let config_path = config::hirsel_dir().join("config.toml");

    let runs_count = if runs_dir.exists() {
        fs::read_dir(&runs_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .count()
    } else {
        0
    };

    let result = execute_reset(target, &runs_dir, &config_path, runs_count)?;

    if json {
        let output = serde_json::json!({
            "success": true,
            "runs_deleted": result.runs_deleted,
            "config_reset": result.config_reset,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(result)
}

fn execute_reset(
    target: ResetTarget,
    runs_dir: &std::path::Path,
    config_path: &std::path::Path,
    runs_count: usize,
) -> anyhow::Result<ResetResult> {
    let mut result = ResetResult {
        runs_deleted: 0,
        config_reset: false,
    };

    // Delete runs
    if matches!(target, ResetTarget::Runs | ResetTarget::All) && runs_dir.exists() {
        fs::remove_dir_all(runs_dir)?;
        fs::create_dir_all(runs_dir)?; // Recreate empty directory
        result.runs_deleted = runs_count;
    }

    // Reset config
    if matches!(target, ResetTarget::Config | ResetTarget::All) {
        if config_path.exists() {
            fs::remove_file(config_path)?;
        }
        // Write minimal default config
        let default_content = r#"# Hirsel configuration
# See hirsel man for all options

[agent]
command = ["hirsel", "__worker-run"]
"#;
        // Ensure hirsel directory exists
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(config_path, default_content)?;
        result.config_reset = true;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reset_target_matching() {
        assert!(matches!(ResetTarget::All, ResetTarget::All));
        assert!(matches!(ResetTarget::Runs, ResetTarget::Runs));
        assert!(matches!(ResetTarget::Config, ResetTarget::Config));
    }
}
