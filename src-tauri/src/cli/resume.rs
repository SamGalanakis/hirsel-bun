//! Implementation of the `hirsel resume` command.
//!
//! Resumes a paused, runaway, or timed-out run by respawning workers.
//!
//! Uses the Orchestrator trait to support both local and remote modes.

use crate::cli::helpers::{
    block_on, get_orchestrator, handle_orchestrator_result_with_data, parse_time_limit, CliOutput,
};
use serde::Serialize;

#[derive(Serialize)]
struct ResumeData {
    #[serde(skip_serializing_if = "Option::is_none")]
    time_limit: Option<String>,
}

/// Run the resume command
pub fn run_resume(run_name: &str, time_limit: Option<&str>, json: bool) -> anyhow::Result<()> {
    run_resume_with_profile(run_name, time_limit, None, json)
}

/// Run the resume command with a specific profile
pub fn run_resume_with_profile(
    run_name: &str,
    time_limit: Option<&str>,
    profile: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let output = CliOutput::new(json);

    // Parse time limit if provided
    let time_limit_minutes = if let Some(limit_str) = time_limit {
        match parse_time_limit(limit_str) {
            Ok(minutes) => Some(minutes as u32),
            Err(e) => {
                output.error_continue(&e);
                return Ok(());
            }
        }
    } else {
        None
    };

    let orch = get_orchestrator(profile)?;

    let time_limit_str = time_limit.map(|s| s.to_string());
    let result = handle_orchestrator_result_with_data(
        block_on(orch.resume_run(run_name, time_limit_minutes)),
        &output,
        json,
        |_| format!("Resumed run '{}'", run_name),
        |_| ResumeData {
            time_limit: time_limit_str.clone(),
        },
    )?;

    if result.is_some() && !json {
        if let Some(limit) = time_limit_minutes {
            println!("Time limit set: {} minutes", limit);
        }
        println!();
        println!("Use 'hirsel attach {}' to watch progress.", run_name);
    }

    Ok(())
}
