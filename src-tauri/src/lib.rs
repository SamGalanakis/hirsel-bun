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
pub use cli::{parse_cli, parse_worker_cli, Cli, Commands, WorkerCli, WorkerCommands};
pub use core::state;
pub use core::Files;
pub use worker::{WorkerConfig, WorkerError, WorkerRunner};

/// Run the CLI commands (called when invoked with arguments)
pub fn run_cli() -> i32 {
    use clap::Parser;
    use cli::*;

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
                println!(
                    "{{\"run_name\": \"{}\", \"run_dir\": \"{}\", \"worker_count\": {}}}",
                    result.run_name,
                    result.run_dir.display(),
                    result.worker_count
                );
            } else {
                println!("Started run: {}", result.run_name);
            }
        }
        Commands::Log(args) => {
            let format = if json {
                OutputFormat::Json
            } else {
                OutputFormat::Pretty
            };
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
                println!(
                    "{{\"run_name\": \"{}\", \"has_changes\": {}}}",
                    result.run_name, result.has_changes
                );
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
        Commands::Clone(_) => {
            // Clone is fully handled by cli/mod.rs
            // This should not be reached via lib.rs
            unreachable!("Clone command should be handled by CLI module");
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
            let run_dir = core::config::run_dir(&args.run_name);
            let files = core::Files::new(&run_dir);
            let state = core::state::SQLiteState::new(files.db_path())
                .map_err(|e| format!("Failed to open database: {}", e))?;
            let hitl = args.new_mode.to_lowercase() == "hitl";
            state
                .set_human_in_the_loop(hitl)
                .map_err(|e| format!("Failed to set mode: {}", e))?;
            if json {
                println!(r#"{{"mode": "{}"}}"#, if hitl { "hitl" } else { "yolo" });
            } else {
                println!("Mode set to {}", if hitl { "hitl" } else { "yolo" });
            }
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
        Commands::Asset(args) => {
            let added = cli::run_asset(&args.run_name, &args.paths)?;
            if json {
                println!("{}", serde_json::json!({ "added": added }));
            } else {
                for file in &added {
                    println!("Added: assets/{}", file);
                }
                println!(
                    "\nReference in spec.md: ![description](assets/{})",
                    added.first().unwrap_or(&String::new())
                );
            }
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
        Commands::Reset(args) => {
            let target = if args.all {
                cli::reset::ResetTarget::All
            } else if args.config {
                cli::reset::ResetTarget::Config
            } else if args.runs {
                cli::reset::ResetTarget::Runs
            } else {
                return Err("Please specify: --runs, --config, or --all".into());
            };

            if let Some(confirm) = &args.confirm {
                if confirm == "reset" {
                    cli::reset::execute_reset_confirmed(target, json)?;
                } else {
                    return Err("Invalid confirmation. Use --confirm reset".into());
                }
            } else {
                cli::reset::run_reset(target, json)?;
            }
        }
        Commands::WorkerRun(args) => {
            // Internal command for worker subprocess
            use std::path::PathBuf;

            let agent_command: Vec<String> = serde_json::from_str(&args.agent_command)
                .map_err(|e| format!("Invalid agent_command JSON: {}", e))?;
            let teammates = args.teammates.map(|t| {
                t.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            });

            let config = worker::WorkerRunConfig {
                run_name: args.run,
                worker_name: args.worker,
                work_dir: PathBuf::from(args.work_dir),
                run_dir: PathBuf::from(args.run_dir),
                spec_path: PathBuf::from(args.spec),
                log_file: PathBuf::from(args.log_file),
                agent_command,
                is_leader: args.is_leader,
                leader_name: args.leader_name,
                teammates,
                resume_session_id: args.resume_session_id,
            };

            // Run the async worker in a tokio runtime
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async {
                tokio::task::LocalSet::new()
                    .run_until(async { worker::run_acp_worker(config).await })
                    .await
            })
            .map_err(|e| format!("Worker error: {}", e))?;
        }
        Commands::EvalMcp => {
            // Internal command for eval MCP server
            worker::run_eval_mcp_server();
        }
        Commands::WorkerMcp => {
            // Internal command for worker MCP server
            worker::run_mcp_server().map_err(|e| format!("Worker MCP error: {}", e))?;
        }
        Commands::EvalRun(args) => {
            // Internal command to run eval agent
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async {
                tokio::task::LocalSet::new()
                    .run_until(async {
                        core::eval::run_eval_from_args(
                            &args.run,
                            &args.run_dir,
                            &args.spec,
                            &args.eval_spec,
                            &args.agent_command,
                        )
                        .await
                    })
                    .await
            })
            .map_err(|e| format!("Eval error: {}", e))?;
        }
        Commands::CompactLearnings(args) => {
            // Internal command to run learnings compaction
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async {
                tokio::task::LocalSet::new()
                    .run_until(async { cli::compact::execute(&args.run_name).await })
                    .await
            })
            .map_err(|e| format!("Compaction error: {}", e))?;
        }
        Commands::RemoteWorker(args) => {
            // Internal command for remote worker subprocess
            let agent_command: Vec<String> = serde_json::from_str(&args.agent_command)
                .map_err(|e| format!("Invalid agent_command JSON: {}", e))?;
            let teammates = args.teammates.map(|t| {
                t.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            });

            // Run the async remote worker
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async {
                tokio::task::LocalSet::new()
                    .run_until(async {
                        worker::run_remote_worker(
                            &args.api_url,
                            &args.run_name,
                            &args.worker_name,
                            &args.work_dir,
                            &args.spec,
                            &agent_command,
                            args.is_leader,
                            args.leader_name.as_deref(),
                            teammates,
                        )
                        .await
                    })
                    .await
            })
            .map_err(|e| format!("Remote worker error: {}", e))?;
        }
        Commands::Test(args) => {
            cli::test::execute(
                args.scenario.as_deref(),
                args.run_name.as_deref(),
                Some(&args.workers),
                args.yolo,
                json,
            )
            .map_err(|e| format!("Test error: {}", e))?;
        }
        // Completion helpers - handled by cli/mod.rs
        Commands::CompleteRuns | Commands::CompleteWorkers(_) | Commands::CompleteThreads(_) => {
            // These are handled by the cli module's run_cli function
            // This code path should not be reached
        }
    }

    Ok(())
}

/// Run the GUI (Tauri application)
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use std::sync::Arc;

    let mut builder = tauri::Builder::default().plugin(tauri_plugin_log::Builder::new().build());

    // Enable MCP plugin in debug builds for AI agent debugging
    #[cfg(debug_assertions)]
    {
        builder = builder.plugin(tauri_plugin_mcp::init_with_config(
            tauri_plugin_mcp::PluginConfig::new("Hirsel".to_string())
                .start_socket_server(true)
                .socket_path("/tmp/hirsel-mcp.sock".into()),
        ));
    }

    // Create chat session manager as shared state
    let chat_manager = Arc::new(core::ChatSessionManager::new());

    builder
        .manage(chat_manager)
        .invoke_handler(gui::get_handlers())
        .setup(|app| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                // Set window background color to match app theme (prevents white flash on resize)
                // Dark background color #1a1a1a = rgb(26, 26, 26)
                let _ = window.set_background_color(Some(tauri::window::Color(26, 26, 26, 255)));
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
