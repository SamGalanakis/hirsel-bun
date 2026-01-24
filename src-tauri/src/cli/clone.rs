//! Clone command - duplicate an existing run
//!
//! Creates a copy of an existing run with a new name, preserving the
//! workspace state but resetting to draft status.

use crate::core::ops::{clone_run, CloneRunConfig, CloneRunResult, OpsError};

/// Error type for clone operations
#[derive(Debug, thiserror::Error)]
pub enum CloneError {
    #[error("Clone failed: {0}")]
    Clone(#[from] OpsError),
}

/// Result type for clone operations
pub type CloneResult<T> = Result<T, CloneError>;

/// Output from a successful clone operation
#[derive(Debug, Clone, serde::Serialize)]
pub struct CloneOutput {
    pub source: String,
    pub new_name: String,
    pub status: &'static str,
}

impl From<CloneRunResult> for CloneOutput {
    fn from(result: CloneRunResult) -> Self {
        Self {
            source: result.source_run,
            new_name: result.new_name,
            status: "draft",
        }
    }
}

/// Execute the clone command
pub fn execute(
    source_run: &str,
    new_name: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = CloneRunConfig::new(source_run, new_name);

    match clone_run(config) {
        Ok(result) => {
            let output = CloneOutput::from(result);
            if json {
                println!("{}", serde_json::to_string(&output)?);
            } else {
                println!(
                    "Cloned '{}' to '{}' (draft)",
                    output.source, output.new_name
                );
            }
            Ok(())
        }
        Err(e) => Err(CloneError::Clone(e).into()),
    }
}
