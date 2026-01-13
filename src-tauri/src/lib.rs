//! Hirsel - Herd your AI coding agents
//!
//! This library provides the core functionality for hirsel,
//! including file system utilities, state management, CLI
//! command routing, and the Tauri GUI integration.

pub mod cli;
pub mod core;
pub mod gui;
pub mod worker;

// Re-export commonly used types
pub use cli::{parse_cli, parse_worker_cli, run_cli, Cli, Commands, WorkerCli, WorkerCommands};
pub use core::Files;
pub use core::state;
pub use worker::{WorkerRunner, WorkerConfig, WorkerError};

/// Run the CLI commands (called when invoked with arguments)
pub fn run_cli() -> i32 {
    use cli::*;
    use clap::Parser;

    let cli = Cli::parse();

    let result = match cli.command {
        None => {
            // No command - show help
            use clap::CommandFactory;
            Cli::command().print_help().ok();
            println!();
            Ok(())
        }
        Some(cmd) => run_command(cmd, cli.json),
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {}", e);
            1
        }
    }
}

/// Execute a CLI command
fn run_command(cmd: Commands, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    use cli::*;

    match cmd {
        Commands::Runs => list_runs(json)?,
        Commands::View(args) => view::execute(&args.run_name, json)?,
        Commands::Go(args) => {
            let result = run_go(&args)?;
            if json {
                // Serialize manually since GoOutput doesn't derive Serialize
                println!("{{\"run_name\": \"{}\", \"run_dir\": \"{}\", \"worker_count\": {}}}",
                    result.run_name, result.run_dir.display(), result.worker_count);
            } else {
                println!("Started run: {}", result.run_name);
            }
        }
        Commands::Log(args) => {
            let format = if json { OutputFormat::Json } else { OutputFormat::Pretty };
            match run_log(&args.run_name, args.follow, args.limit, format) {
                log::LogResult::Success => {}
                log::LogResult::Empty => println!("No activity yet"),
                log::LogResult::Interrupted => {}
                log::LogResult::Error(e) => return Err(e.into()),
            }
        }
        Commands::Attach(args) => {
            run_attach(&args.run_name, args.target.as_deref(), json)?;
        }
        Commands::Msg(args) => {
            if args.list_threads {
                let threads = get_available_threads(&args.run_name)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&threads)?);
                } else {
                    for thread in threads {
                        println!("{}", thread);
                    }
                }
            } else {
                let result = run_msg(&MsgArgs {
                    run_name: args.run_name.clone(),
                    message: args.message.clone(),
                    thread: args.thread.clone(),
                    list_threads: false,
                })?;
                // MsgOutput doesn't implement Serialize, just print success
                match result {
                    MsgOutput::Sent { thread, .. } => {
                        if json {
                            println!("{{\"sent\": true, \"thread\": \"{}\"}}", thread);
                        } else {
                            println!("Message sent to {}", thread);
                        }
                    }
                    MsgOutput::Messages { messages, .. } => {
                        for msg in messages {
                            println!("[{}] {}: {}", msg.timestamp, msg.sender, msg.content);
                        }
                    }
                    MsgOutput::ThreadList { threads, .. } => {
                        for thread in threads {
                            println!("{}: {} messages", thread.name, thread.message_count);
                        }
                    }
                }
            }
        }
        Commands::Diff(args) => {
            let result = cli::diff::run_diff(&args.run_name, false)?;
            if json {
                println!("{{\"run_name\": \"{}\", \"has_changes\": {}}}",
                    result.run_name, result.has_changes);
            } else if let Some(diff) = result.diff {
                println!("{}", diff);
            } else {
                println!("No changes");
            }
        }
        Commands::Deliver(args) => {
            cli::deliver::execute(&args.run_name, args.branch.as_deref(), json)?;
        }
        Commands::Pause(args) => {
            run_pause(&args.run_name, json)?;
        }
        Commands::Resume(args) => {
            run_resume(&args.run_name, args.time_limit.as_deref(), json)?;
        }
        Commands::Delete(args) => {
            run_delete(&args.run_name, json)?;
        }
        Commands::Prune => {
            run_prune(json)?;
        }
        Commands::Summary(args) => {
            let output = run_summary(&args.run_name, args.regenerate, json)?;
            if !json {
                println!("{}", output);
            }
        }
        Commands::Mode(args) => {
            cli::view::execute(&args.run_name, json)?; // TODO: implement mode change
        }
        Commands::Amend(args) => {
            let run_dir = core::config::run_dir(&args.run_name);
            // Create a new amendment with current timestamp
            let amendment = Amendment {
                id: chrono::Utc::now().timestamp(),
                timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                message: args.message.clone(),
            };
            update_spec_amendments(&run_dir, &[amendment])?;
            if !json {
                println!("Amendment added to run '{}'", args.run_name);
            }
        }
        Commands::Spec(args) => {
            let run_dir = core::config::run_dir(&args.run_name);
            let spec = run_spec(&run_dir)?;
            println!("{}", spec);
        }
        Commands::Tasks(args) => {
            let output = cli::tasks::run_tasks(&args.run_name, json)?;
            println!("{}", output);
        }
        Commands::TaskAdd(args) => {
            let output = cli::tasks::run_task_add(
                &args.run_name,
                &args.task_id,
                &args.description,
                args.parent.as_deref(),
                &args.blocked_by,
                json,
            )?;
            print!("{}", output);
        }
        Commands::TaskDelete(args) => {
            let output = cli::tasks::run_task_delete(&args.run_name, &args.task_id, json)?;
            print!("{}", output);
        }
        Commands::TaskDone(args) => {
            let output = cli::tasks::run_task_done(&args.run_name, &args.task_id, json)?;
            print!("{}", output);
        }
        Commands::TaskReopen(args) => {
            let output = cli::tasks::run_task_reopen(&args.run_name, &args.task_id, json)?;
            print!("{}", output);
        }
        Commands::TaskUnclaim(args) => {
            let output = cli::tasks::run_task_unclaim(&args.run_name, &args.task_id, json)?;
            print!("{}", output);
        }
        Commands::Config(args) => {
            run_config(args.agent)?;
        }
        Commands::Templates => {
            let output = run_templates(json)?;
            println!("{}", output);
        }
        Commands::Completions(args) => {
            run_completions(&args)?;
        }
        Commands::Man(args) => {
            run_man(&args)?;
        }
        Commands::Improve(args) => {
            cli::improve::execute(args.run_name.as_deref(), json)?;
        }
    }

    Ok(())
}

/// Run the GUI (Tauri application)
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .invoke_handler(gui::get_handlers())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
