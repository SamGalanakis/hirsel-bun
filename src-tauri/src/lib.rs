//! Hirsel - Herd your AI coding agents
//!
//! This library provides the core functionality for hirsel,
//! including file system utilities, state management, CLI
//! command routing, and the Tauri GUI integration.

pub mod cli;
pub mod core;
pub mod daemon;
#[cfg(feature = "gui")]
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
        Some(cmd) => run_command(cmd, cli.json, cli.profile.as_deref()),
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
fn run_command(
    cmd: Commands,
    json: bool,
    _profile: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
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
                agent_command,
                is_leader: args.is_leader,
                leader_name: args.leader_name,
                teammates,
                resume_session_id: args.resume_session_id,
            };

            // Run the async worker in a tokio runtime with signal handling
            let worker_name = config.worker_name.clone();
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async {
                tokio::task::LocalSet::new()
                    .run_until(async {
                        #[cfg(unix)]
                        {
                            use tokio::signal::unix::{signal, SignalKind};
                            let mut sigterm = signal(SignalKind::terminate())
                                .expect("Failed to register SIGTERM handler");

                            tokio::select! {
                                result = worker::run_acp_worker(config) => {
                                    // Normal completion - cleanup already happens in run_acp_worker
                                    result
                                }
                                _ = sigterm.recv() => {
                                    // Received SIGTERM from GUI - ensure cleanup
                                    tracing::info!("[{}] Received SIGTERM, cleaning up process group", worker_name);
                                    core::process::cleanup_process_group(&worker_name);
                                    Ok(())
                                }
                            }
                        }
                        #[cfg(not(unix))]
                        {
                            worker::run_acp_worker(config).await
                        }
                    })
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
                args.remote.as_deref(),
                args.runner.as_deref(),
            )
            .map_err(|e| format!("Test error: {}", e))?;
        }
        Commands::Serve(args) => {
            // Server mode - run HTTP server for remote orchestration
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async { core::server::start_server(args.port).await })
                .map_err(|e| format!("Server error: {}", e))?;
        }
        Commands::Daemon(args) => {
            // Internal daemon command - runs the daemon server
            use daemon::{start_daemon, DaemonConfig};

            let config = DaemonConfig {
                idle_timeout_secs: args.idle_timeout,
            };

            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async { start_daemon(config).await })
                .map_err(|e| format!("Daemon error: {}", e))?;
        }
        Commands::DaemonCtl(args) => {
            // Daemon control commands
            use cli::DaemonCommand;

            match args.command {
                DaemonCommand::Start => {
                    if daemon::is_daemon_running() {
                        if json {
                            println!(r#"{{"status": "already_running"}}"#);
                        } else {
                            println!("Daemon is already running");
                        }
                    } else {
                        // Start daemon by connecting (which auto-starts)
                        let rt = tokio::runtime::Runtime::new()
                            .map_err(|e| format!("Failed to create runtime: {}", e))?;
                        let client = daemon::DaemonClient::connect_or_start()
                            .map_err(|e| format!("Failed to start daemon: {}", e))?;
                        rt.block_on(async { client.health().await })
                            .map_err(|e| format!("Daemon health check failed: {}", e))?;
                        if json {
                            println!(r#"{{"status": "started"}}"#);
                        } else {
                            println!("Daemon started");
                        }
                    }
                }
                DaemonCommand::Stop => {
                    if !daemon::is_daemon_running() {
                        if json {
                            println!(r#"{{"status": "not_running"}}"#);
                        } else {
                            println!("Daemon is not running");
                        }
                    } else {
                        let rt = tokio::runtime::Runtime::new()
                            .map_err(|e| format!("Failed to create runtime: {}", e))?;
                        let client = daemon::DaemonClient::connect()
                            .map_err(|e| format!("Failed to connect to daemon: {}", e))?;
                        rt.block_on(async { client.stop().await })
                            .map_err(|e| format!("Failed to stop daemon: {}", e))?;
                        if json {
                            println!(r#"{{"status": "stopped"}}"#);
                        } else {
                            println!("Daemon stopped");
                        }
                    }
                }
                DaemonCommand::Status => {
                    if daemon::is_daemon_running() {
                        if json {
                            // Get full status from daemon
                            let rt = tokio::runtime::Runtime::new()
                                .map_err(|e| format!("Failed to create runtime: {}", e))?;
                            let client = daemon::DaemonClient::connect()
                                .map_err(|e| format!("Failed to connect to daemon: {}", e))?;
                            let status: serde_json::Value = rt
                                .block_on(async { client.get("/daemon/status").await })
                                .map_err(|e| format!("Failed to get status: {}", e))?;
                            println!("{}", serde_json::to_string_pretty(&status).unwrap());
                        } else {
                            println!("Daemon is running");
                            println!("Socket: {}", daemon::socket_path().display());
                        }
                    } else {
                        if json {
                            println!(r#"{{"running": false}}"#);
                        } else {
                            println!("Daemon is not running");
                        }
                    }
                }
            }
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
#[cfg(feature = "gui")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use std::sync::Arc;

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // When a second instance tries to launch, focus the existing window
            use tauri::Manager;
            tracing::info!("Second instance attempted with args: {:?}", args);
            if let Some(window) = app.get_webview_window("main") {
                // Unminimize if minimized, then focus
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }));

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

    // Create chat orchestrator manager from the session manager
    let chat_orchestrator_manager = Arc::new(gui::ChatOrchestratorManager::from_manager(
        chat_manager.clone(),
    ));

    // Create worker event stream manager as shared state
    let worker_stream_manager = Arc::new(gui::WorkerEventStreamManager::new());

    builder
        .manage(chat_manager.clone())
        .manage(chat_orchestrator_manager)
        .manage(worker_stream_manager)
        .invoke_handler(gui::get_handlers())
        .setup(|app| {
            use tauri::Manager;

            // In dev mode, clean up orphaned processes from previous hot-reload sessions
            #[cfg(debug_assertions)]
            {
                cleanup_orphaned_dev_processes();
            }

            // Reconcile stale workers on startup (both dev and release)
            // Workers that appear "Working" but have dead PIDs are marked as Paused
            let stale = core::workers::reconcile_stale_workers();
            if !stale.is_empty() {
                tracing::info!(
                    "[GUI] Startup reconciliation: marked {} stale worker(s) as Paused",
                    stale.len()
                );
            }

            if let Some(window) = app.get_webview_window("main") {
                // Set window background color to match app theme (prevents white flash on resize)
                // Dark background color #1a1a1a = rgb(26, 26, 26)
                let _ = window.set_background_color(Some(tauri::window::Color(26, 26, 26, 255)));
            }
            Ok(())
        })
        .on_window_event(move |window, event| {
            // Clean up when the main window is about to close
            if let tauri::WindowEvent::Destroyed = event {
                if window.label() == "main" {
                    tracing::info!("[GUI] Main window destroyed, cleaning up all processes");
                    cleanup_all_processes(&chat_manager);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Kill all worker and chat session processes on GUI exit
#[cfg(feature = "gui")]
fn cleanup_all_processes(chat_manager: &std::sync::Arc<core::ChatSessionManager>) {
    tracing::info!("[GUI] Killing all worker processes");

    // Get the runs directory
    let runs_dir = match dirs::home_dir() {
        Some(home) => home.join(".hirsel").join("runs"),
        None => {
            tracing::warn!("[GUI] Could not determine home directory");
            stop_chat_sessions(chat_manager);
            return;
        }
    };

    // Iterate over all run directories and open their databases
    if let Ok(entries) = std::fs::read_dir(&runs_dir) {
        let mut all_pids: Vec<i64> = Vec::new();

        for entry in entries.flatten() {
            let db_path = entry.path().join("hirsel.db");
            if db_path.exists() {
                if let Ok(state) = core::state::SQLiteState::new(db_path) {
                    if let Ok(workers) = state.get_workers() {
                        for worker in workers {
                            if let Some(pid) = worker.pid {
                                tracing::info!(
                                    "[GUI] Found worker {} (pid {}) in {}",
                                    worker.name,
                                    pid,
                                    entry.path().display()
                                );
                                all_pids.push(pid);
                            }
                        }
                    }
                }
            }
        }

        // Kill all found worker process groups
        #[cfg(unix)]
        {
            // First SIGTERM
            for &pid in &all_pids {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGTERM);
                }
            }

            // Brief wait then force kill
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Then SIGKILL
            for &pid in &all_pids {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
        }

        tracing::info!("[GUI] Killed {} worker process groups", all_pids.len());
    }

    // Stop all active chat sessions
    stop_chat_sessions(chat_manager);

    tracing::info!("[GUI] Cleanup complete");
}

#[cfg(feature = "gui")]
fn stop_chat_sessions(chat_manager: &std::sync::Arc<core::ChatSessionManager>) {
    tracing::info!("[GUI] Stopping all chat sessions");
    let rt = tokio::runtime::Runtime::new();
    if let Ok(rt) = rt {
        rt.block_on(async {
            let sessions = chat_manager.list_sessions().await;
            for session_id in sessions {
                let _ = chat_manager.stop_session(&session_id).await;
            }
        });
    }
}

/// Clean up orphaned claude-code-acp processes from previous dev sessions.
/// This is only compiled in debug builds to handle hot-reload orphans.
#[cfg(all(feature = "gui", debug_assertions))]
fn cleanup_orphaned_dev_processes() {
    use std::process::Command;

    tracing::info!("[DEV] Cleaning up orphaned claude-code-acp processes from previous sessions");

    // Kill all claude-code-acp processes - they're orphans from previous hot-reload
    match Command::new("pkill")
        .args(["-f", "claude-code-acp"])
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                tracing::info!("[DEV] Killed orphaned claude-code-acp processes");
            } else {
                // Exit code 1 means no processes matched - that's fine
                tracing::debug!("[DEV] No orphaned claude-code-acp processes found");
            }
        }
        Err(e) => {
            tracing::warn!("[DEV] Failed to run pkill: {}", e);
        }
    }
}
