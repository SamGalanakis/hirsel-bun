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

/// Initialize tracing subscriber with profiling support.
/// When built with `--features profiling` AND HIRSEL_PROFILING=1, outputs Chrome Trace Format
/// JSON to `~/.hirsel/profiling/trace-{timestamp}.json` for viewing in Perfetto UI.
#[cfg(feature = "profiling")]
fn init_tracing() {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::from_default_env()
        .add_directive("sqlx=off".parse().unwrap())
        .add_directive("rustls=warn".parse().unwrap())
        .add_directive("rustls_platform_verifier=warn".parse().unwrap())
        .add_directive("hyper=warn".parse().unwrap())
        .add_directive("reqwest=warn".parse().unwrap());

    if std::env::var("HIRSEL_PROFILING").as_deref() == Ok("1") {
        let profiling_dir = std::env::var("HIRSEL_PROFILING_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                let dir = core::hirsel_dir()
                    .join("profiling")
                    .join(chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string());
                dir
            });
        std::fs::create_dir_all(&profiling_dir).expect("Failed to create profiling directory");

        let trace_filename =
            std::env::var("HIRSEL_TRACE_FILENAME").unwrap_or_else(|_| "trace.json".to_string());
        let trace_file = profiling_dir.join(trace_filename);
        eprintln!("[profiling] Writing trace to {}", trace_file.display());

        let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
            .file(trace_file)
            .include_args(true)
            .build();

        // Store guard in a static so traces flush on process exit.
        // FlushGuard is !Sync, so we use Mutex instead of OnceLock.
        static FLUSH_GUARD: std::sync::Mutex<Option<tracing_chrome::FlushGuard>> =
            std::sync::Mutex::new(None);
        *FLUSH_GUARD.lock().unwrap() = Some(guard);

        let fmt_layer = tracing_subscriber::fmt::layer().with_filter(filter);
        let _ = tracing_subscriber::registry()
            .with(fmt_layer)
            .with(chrome_layer)
            .try_init();
    } else {
        let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    }
}

/// Initialize tracing subscriber for debug logging.
/// Only active when built with `--features dev` AND RUST_LOG is set.
#[cfg(all(feature = "dev", not(feature = "profiling")))]
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::from_default_env()
        .add_directive("sqlx=warn".parse().unwrap())
        .add_directive("rustls=warn".parse().unwrap())
        .add_directive("rustls_platform_verifier=warn".parse().unwrap())
        .add_directive("hyper=warn".parse().unwrap())
        .add_directive("reqwest=warn".parse().unwrap());
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

/// No-op tracing init when dev feature is not enabled.
#[cfg(not(feature = "dev"))]
fn init_tracing() {}

// Re-export commonly used types
pub use cli::{
    parse_cli, parse_worker_cli, Cli, Commands, MsgSubcommands, TaskSubcommands, WorkerCli,
    WorkerCommands,
};
pub use core::state;
pub use core::Files;
pub use worker::{WorkerConfig, WorkerError, WorkerRunner};

/// Run the CLI commands (called when invoked with arguments)
pub fn run_cli() -> i32 {
    init_tracing();

    use clap::Parser;

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
        Some(cmd) => run_command(cmd),
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
fn run_command(cmd: Commands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        Commands::WorkerRun(args) => {
            // Internal command for worker subprocess
            use std::path::PathBuf;

            // Set env vars for child processes (MCP server, agent)
            // These are passed as CLI args to avoid duplication, but child processes need env vars
            std::env::set_var("HIRSEL_RUN", &args.run);
            std::env::set_var("HIRSEL_WORKER", &args.worker);
            std::env::set_var("HIRSEL_RUN_DIR", &args.run_dir);

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
                assigned_task_id: args.assigned_task_id,
                is_plan_task: args.plan_task,
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
        Commands::BoardMcp => {
            // Run board MCP server for Shepherd
            let project_id: i64 = std::env::var("HIRSEL_PROJECT_ID")
                .map_err(|_| "HIRSEL_PROJECT_ID environment variable required")?
                .parse()
                .map_err(|_| "Invalid HIRSEL_PROJECT_ID")?;
            core::board::mcp::run_board_mcp_server(project_id)
                .map_err(|e| format!("Board MCP error: {}", e))?;
        }
        #[cfg(feature = "server")]
        Commands::Serve(args) => {
            // Server mode - run the backend HTTP server
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
    }

    Ok(())
}

/// Run the GUI (Tauri application)
#[cfg(feature = "gui")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    let mut builder = tauri::Builder::default();

    // When profiling, init_tracing() already set up a global subscriber (fmt + chrome),
    // so skip tauri_plugin_log to avoid "logger already initialized" panic.
    #[cfg(feature = "profiling")]
    let skip_tauri_log = std::env::var("HIRSEL_PROFILING").as_deref() == Ok("1");
    #[cfg(not(feature = "profiling"))]
    let skip_tauri_log = false;

    if !skip_tauri_log {
        builder = builder.plugin(
            tauri_plugin_log::Builder::new()
                .filter(|metadata| {
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
        );
    }

    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        // When a second instance tries to launch, focus the existing window
        use tauri::Manager;
        tracing::info!("Second instance attempted with args: {:?}", args);
        if let Some(window) = app.get_webview_window("main") {
            // Unminimize if minimized, then focus
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }));

    // Create worker event stream manager as shared state
    let worker_stream_manager = std::sync::Arc::new(gui::WorkerEventStreamManager::new());

    builder
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
            // Workers continue running when the UI closes - they are daemon-managed.
            if let tauri::WindowEvent::Destroyed = event {
                if window.label() == "main" {
                    tracing::info!("[GUI] Main window closed");
                    cleanup_all_processes();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Clean up GUI-specific processes on exit.
///
/// Workers are not killed when the UI closes - they continue running and are
/// managed by the daemon.
#[cfg(feature = "gui")]
fn cleanup_all_processes() {
    tracing::info!("[GUI] Main window closing, cleaning up GUI processes");
    tracing::info!("[GUI] Cleanup complete (workers continue running)");
}

/// Clean up leftover worker helper processes from previous dev sessions.
/// This is only compiled in debug builds to handle hot-reload orphans.
#[cfg(all(feature = "gui", debug_assertions))]
fn cleanup_orphaned_dev_processes() {
    use std::process::Command;

    tracing::info!("[DEV] Cleaning up orphaned worker helper processes from previous sessions");

    // Kill old hidden helper commands from previous hot-reload sessions.
    match Command::new("pkill").args(["-f", "__worker-run"]).output() {
        Ok(output) => {
            if output.status.success() {
                tracing::info!("[DEV] Killed orphaned worker helper processes");
            } else {
                // Exit code 1 means no processes matched - that's fine
                tracing::debug!("[DEV] No orphaned worker helper processes found");
            }
        }
        Err(e) => {
            tracing::warn!("[DEV] Failed to run pkill: {}", e);
        }
    }
}
