//! Hirsel - Herd your AI coding agents
//!
//! This library provides the core functionality for hirsel,
//! including file system utilities, state management, CLI
//! command routing, and the Tauri GUI integration.

// Allow these clippy warnings crate-wide
#![allow(clippy::should_implement_trait)] // from_str methods are intentional
#![allow(clippy::too_many_arguments)] // Complex functions need many args
#![allow(clippy::ptr_arg)] // &PathBuf is fine for owned paths

pub mod cli;
pub mod core;
#[cfg(feature = "server")]
pub mod daemon;
#[cfg(feature = "gui")]
pub mod gui;
pub mod version;
pub mod worker;

/// Initialize tracing subscriber for debug logging.
/// Only active when built with `--features dev` AND RUST_LOG is set.
#[cfg(feature = "dev")]
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    // Build filter from RUST_LOG env, but suppress noisy library spam
    let filter = EnvFilter::from_default_env()
        .add_directive("sqlx=warn".parse().unwrap())
        .add_directive("rustls=warn".parse().unwrap())
        .add_directive("rustls_platform_verifier=warn".parse().unwrap())
        .add_directive("hyper=warn".parse().unwrap())
        .add_directive("reqwest=warn".parse().unwrap());
    // Only initialize once, ignore errors from multiple calls
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

/// No-op tracing init when dev feature is not enabled.
#[cfg(not(feature = "dev"))]
fn init_tracing() {}

// Re-export commonly used types
pub use cli::{parse_cli, parse_worker_cli, Cli, Commands, WorkerCli, WorkerCommands};
pub use core::state;
pub use core::Files;
pub use worker::{WorkerConfig, WorkerError, WorkerRunner};

/// Run the CLI commands (called when invoked with arguments)
pub fn run_cli() -> i32 {
    init_tracing();

    use clap::Parser;
    use cli::*;

    let cli = Cli::parse();

    // Handle --build-info flag
    if cli.build_info {
        println!("{}", version::build_info());
        return 0;
    }

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
        #[cfg(feature = "cli")]
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
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async {
                let state = core::state::SQLiteState::new(&args.run_name)
                    .await
                    .map_err(|e| format!("Failed to open database: {}", e))?;
                let hitl = args.new_mode.to_lowercase() == "hitl";
                state
                    .set_human_in_the_loop(hitl)
                    .await
                    .map_err(|e| format!("Failed to set mode: {}", e))?;
                Ok::<(), String>(())
            })?;
            let hitl = args.new_mode.to_lowercase() == "hitl";
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

            // Set env vars for child processes (MCP server, agent)
            // These are passed as CLI args to avoid duplication, but child processes need env vars
            std::env::set_var("HIRSEL_RUN", &args.run);
            std::env::set_var("HIRSEL_WORKER", &args.worker);
            std::env::set_var("HIRSEL_RUN_DIR", &args.run_dir);
            if let Some(ref url) = args.api_url {
                std::env::set_var("HIRSEL_API_URL", url);
            }

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
                agent_command,
                is_leader: args.is_leader,
                leader_name: args.leader_name,
                teammates,
                resume_session_id: args.resume_session_id,
                api_url: args.api_url,
                assigned_task_id: args.assigned_task_id,
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
                                result = worker::run_worker(config) => {
                                    // Normal completion - cleanup already happens in run_worker
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
                            worker::run_worker(config).await
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
                            &args.agent_command,
                        )
                        .await
                    })
                    .await
            })
            .map_err(|e| format!("Eval error: {}", e))?;
        }
        Commands::Scribe(args) => {
            // Internal command to run scribe processing
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async {
                tokio::task::LocalSet::new()
                    .run_until(async { cli::scribe::execute(&args.run_name).await })
                    .await
            })
            .map_err(|e| format!("Scribe error: {}", e))?;
        }
        Commands::ServiceWorker(args) => {
            // Internal command to run service worker (scribe/gyp HTTP server)
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async {
                cli::service_worker::execute(&args.r#type, args.idle_timeout, args.port).await
            })
            .map_err(|e| format!("Service worker error: {}", e))?;
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
                        worker::run_remote_worker_with_config(worker::RemoteWorkerConfig {
                            api_url: &args.api_url,
                            run_name: &args.run_name,
                            worker_name: &args.worker_name,
                            work_dir: &args.work_dir,
                            agent_command: &agent_command,
                            is_leader: args.is_leader,
                            leader_name: args.leader_name.as_deref(),
                            teammates,
                            wait_for_files: args.wait_for_files,
                            file_receiver_port: args.file_receiver_port,
                            assigned_task_id: args.assigned_task_id,
                        })
                        .await
                    })
                    .await
            })
            .map_err(|e| format!("Remote worker error: {}", e))?;
        }
        Commands::AcpBridge => {
            // Run ACP bridge server for Claude CLI
            cli::acp_bridge::run_acp_bridge().map_err(|e| format!("ACP bridge error: {}", e))?;
        }
        Commands::BoardMcp => {
            // Run board MCP server for Gyp
            let project_id: i64 = std::env::var("HIRSEL_PROJECT_ID")
                .map_err(|_| "HIRSEL_PROJECT_ID environment variable required")?
                .parse()
                .map_err(|_| "Invalid HIRSEL_PROJECT_ID")?;
            core::board::mcp::run_board_mcp_server(project_id)
                .map_err(|e| format!("Board MCP error: {}", e))?;
        }
        #[cfg(feature = "server")]
        Commands::Serve(args) => {
            // Server mode - run HTTP server for remote orchestration
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async { core::server::start_server(args.port).await })
                .map_err(|e| format!("Server error: {}", e))?;
        }
        #[cfg(feature = "server")]
        Commands::Daemon(args) => {
            // Internal daemon command - runs the daemon server
            use daemon::{start_daemon, DaemonConfig};

            let config = DaemonConfig {
                idle_timeout_secs: args.idle_timeout,
                tcp_port: args.tcp_port,
            };

            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async { start_daemon(config).await })
                .map_err(|e| format!("Daemon error: {}", e))?;
        }
        #[cfg(feature = "server")]
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
                            println!("Port: {}", daemon::DEFAULT_TCP_PORT);
                        }
                    } else if json {
                        println!(r#"{{"running": false}}"#);
                    } else {
                        println!("Daemon is not running");
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
    init_tracing();

    use std::sync::Arc;

    let builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .filter(|metadata| {
                    // Filter out noisy library spam
                    let target = metadata.target();
                    !target.starts_with("rustls")
                        && !target.starts_with("hyper")
                        && !target.starts_with("reqwest")
                        && !target.starts_with("zbus")
                        && !target.starts_with("mio")
                        && !target.starts_with("tracing::span")
                        && !target.starts_with("sqlx")
                })
                .build(),
        )
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
            // Create a runtime since we're in a sync context (tauri setup, no runtime yet)
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            let stale = rt.block_on(core::workers::reconcile_stale_workers());
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
            // Clean up GUI-specific processes when main window closes
            // Workers continue running - they are managed by the daemon
            if let tauri::WindowEvent::Destroyed = event {
                if window.label() == "main" {
                    tracing::info!("[GUI] Main window closed");
                    cleanup_all_processes(&chat_manager);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Clean up GUI-specific processes on exit (chat sessions only)
///
/// Workers are NOT killed when the UI closes - they continue running and are
/// managed by the daemon. This allows users to close the UI while work continues.
#[cfg(feature = "gui")]
fn cleanup_all_processes(chat_manager: &std::sync::Arc<core::ChatSessionManager>) {
    tracing::info!("[GUI] Main window closing, cleaning up GUI processes");

    // Stop all active chat sessions (these are GUI-specific)
    stop_chat_sessions(chat_manager);

    // Workers continue running - they are managed by the daemon
    tracing::info!("[GUI] Cleanup complete (workers continue running)");
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

/// Clean up orphaned hirsel __acp-bridge processes from previous dev sessions.
/// This is only compiled in debug builds to handle hot-reload orphans.
#[cfg(all(feature = "gui", debug_assertions))]
fn cleanup_orphaned_dev_processes() {
    use std::process::Command;

    tracing::info!("[DEV] Cleaning up orphaned acp-bridge processes from previous sessions");

    // Kill all hirsel __acp-bridge processes - they're orphans from previous hot-reload
    match Command::new("pkill").args(["-f", "__acp-bridge"]).output() {
        Ok(output) => {
            if output.status.success() {
                tracing::info!("[DEV] Killed orphaned acp-bridge processes");
            } else {
                // Exit code 1 means no processes matched - that's fine
                tracing::debug!("[DEV] No orphaned acp-bridge processes found");
            }
        }
        Err(e) => {
            tracing::warn!("[DEV] Failed to run pkill: {}", e);
        }
    }
}
