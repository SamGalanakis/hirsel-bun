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
mod lash_tools;
pub mod version;
pub mod worker;

#[cfg(feature = "gui")]
use crate::core::orchestrator::Orchestrator;
#[cfg(feature = "gui")]
use tauri::{WebviewUrl, WebviewWindowBuilder};

/// Initialize tracing subscriber with profiling support.
/// When built with `--features profiling` AND HIRSEL_PROFILING=1, outputs Chrome Trace Format
/// JSON to `~/.hirsel/profiling/trace-{timestamp}.json` for viewing in Perfetto UI.
fn init_tracing() {
    use std::fs;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::EnvFilter;

    static FILE_GUARD: OnceLock<Mutex<Option<tracing_appender::non_blocking::WorkerGuard>>> =
        OnceLock::new();

    fn process_role() -> &'static str {
        match std::env::args().nth(1).as_deref() {
            Some("serve") => "server",
            Some("__daemon") | Some("daemon") => "daemon",
            Some("__worker-runtime") | Some("worker-runtime") => "worker",
            Some("worker-mcp") | Some("eval-mcp") => "worker",
            Some("scribe") => "scribe",
            _ => "gui",
        }
    }

    fn cleanup_old_logs(dir: &Path, keep_files: usize) {
        let Ok(read_dir) = fs::read_dir(dir) else {
            return;
        };

        let mut files = read_dir
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let path = entry.path();
                let metadata = entry.metadata().ok()?;
                if !metadata.is_file() {
                    return None;
                }
                let modified = metadata.modified().ok()?;
                Some((path, modified))
            })
            .collect::<Vec<_>>();

        files.sort_by(|a, b| b.1.cmp(&a.1));
        for (path, _) in files.into_iter().skip(keep_files) {
            let _ = fs::remove_file(path);
        }
    }

    fn default_filter() -> EnvFilter {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("hirsel=info,sqlx=warn,rustls=warn,rustls_platform_verifier=warn,hyper=warn,reqwest=warn")
        })
    }

    let role = process_role();
    let logs_dir = core::hirsel_dir().join("logs").join(role);
    if let Err(error) = fs::create_dir_all(&logs_dir) {
        eprintln!(
            "[hirsel] failed to create log directory {}: {}",
            logs_dir.display(),
            error
        );
    }
    let keep_files = std::env::var("HIRSEL_LOG_KEEP_FILES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(14);
    cleanup_old_logs(&logs_dir, keep_files);

    let log_prefix = format!("{}.log", role);
    let file_appender = tracing_appender::rolling::daily(&logs_dir, log_prefix);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    *FILE_GUARD.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(guard);

    let stdout_filter = default_filter();
    let file_filter = default_filter();
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_filter(stdout_filter);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_writer(non_blocking)
        .with_filter(file_filter);

    #[cfg(feature = "profiling")]
    {
        if std::env::var("HIRSEL_PROFILING").as_deref() == Ok("1") {
            let profiling_dir = std::env::var("HIRSEL_PROFILING_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    core::hirsel_dir()
                        .join("profiling")
                        .join(chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string())
                });
            fs::create_dir_all(&profiling_dir).expect("Failed to create profiling directory");

            let trace_filename =
                std::env::var("HIRSEL_TRACE_FILENAME").unwrap_or_else(|_| "trace.json".to_string());
            let trace_file = profiling_dir.join(trace_filename);
            eprintln!("[profiling] Writing trace to {}", trace_file.display());
            eprintln!("[hirsel] {} logs -> {}", role, logs_dir.display());

            let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
                .file(trace_file)
                .include_args(true)
                .build();

            static FLUSH_GUARD: Mutex<Option<tracing_chrome::FlushGuard>> = Mutex::new(None);
            *FLUSH_GUARD.lock().unwrap() = Some(guard);

            let _ = tracing_subscriber::registry()
                .with(stdout_layer)
                .with(file_layer)
                .with(chrome_layer)
                .try_init();
            return;
        }
    }

    eprintln!("[hirsel] {} logs -> {}", role, logs_dir.display());
    let _ = tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .try_init();
}

// Re-export commonly used types
pub use cli::{
    parse_cli, parse_worker_cli, Cli, Commands, TaskSubcommands, WorkerCli, WorkerCommands,
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
        Commands::WorkerRuntime(args) => {
            // Internal command for worker subprocess
            use std::path::PathBuf;

            // Set env vars for child processes (MCP server, agent)
            // These are passed as CLI args to avoid duplication, but child processes need env vars
            std::env::set_var("HIRSEL_RUNTIME", &args.runtime);
            std::env::set_var("HIRSEL_WORKER", &args.worker);
            std::env::set_var("HIRSEL_RUNTIME_DIR", &args.runtime_dir);

            let agent_command: Vec<String> = serde_json::from_str(&args.agent_command)
                .map_err(|e| format!("Invalid agent_command JSON: {}", e))?;
            let teammates = args.teammates.map(|t| {
                t.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            });

            let config = worker::WorkerRunConfig {
                runtime_name: args.runtime,
                worker_name: args.worker,
                work_dir: PathBuf::from(args.work_dir),
                runtime_dir: PathBuf::from(args.runtime_dir),
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
        Commands::EvalRuntime(args) => {
            // Internal command to run eval agent
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {}", e))?;
            rt.block_on(async {
                tokio::task::LocalSet::new()
                    .run_until(async {
                        core::eval::run_eval_from_args(
                            &args.runtime,
                            &args.runtime_dir,
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
                    .run_until(async { cli::scribe::execute(&args.runtime_name).await })
                    .await
            })
            .map_err(|e| format!("Scribe error: {}", e))?;
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

    let builder =
        tauri::Builder::default().plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // When a second instance tries to launch, focus the existing window
            use tauri::Manager;
            tracing::info!("Second instance attempted with args: {:?}", args);
            if let Some(window) = app.get_webview_window("main") {
                // Unminimize if minimized, then focus
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }));

    builder
        .invoke_handler(gui::get_handlers())
        .setup(|app| {
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

            create_main_window(app.handle())?;
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

#[cfg(feature = "gui")]
fn create_main_window(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let (config, _) =
        core::config::Config::load().unwrap_or_else(|_| (core::config::Config::default(), vec![]));
    let mut window_config = app
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or("missing main window config")?;

    let mut initial_url = WebviewUrl::App("index.html".into());
    let backend = config.backend.clone();
    if let (Some(url), Some(api_key)) = (backend.url, backend.api_key) {
        if !url.trim().is_empty() && !api_key.trim().is_empty() {
            let rt = tokio::runtime::Runtime::new()
                .expect("Failed to create tokio runtime for backend bootstrap");
            let should_open = rt.block_on(async {
                let orchestrator =
                    core::orchestrator::RemoteOrchestrator::new(url.clone(), api_key.clone());
                orchestrator.health().await.is_ok()
            });

            if should_open {
                let mut bootstrap = format!("{}/connect/bootstrap", url.trim_end_matches('/'))
                    .parse::<tauri::Url>()?;
                bootstrap
                    .query_pairs_mut()
                    .append_pair("api_key", &api_key)
                    .append_pair("return_to", "/app");
                initial_url = WebviewUrl::External(bootstrap);
            }
        }
    }

    window_config.create = true;
    window_config.url = initial_url;
    let window = WebviewWindowBuilder::from_config(app, &window_config)?.build()?;
    let _ = window.set_background_color(Some(tauri::window::Color(26, 26, 26, 255)));
    Ok(())
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
    match Command::new("pkill")
        .args(["-f", "__worker-runtime"])
        .output()
    {
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
