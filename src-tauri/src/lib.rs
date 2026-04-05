//! Hirsel - Herd your AI coding agents
//!
//! This library provides Hirsel's shared backend/runtime code and the
//! thin Tauri desktop host.

// Allow these clippy warnings crate-wide
#![allow(clippy::should_implement_trait)] // from_str methods are intentional
#![allow(clippy::too_many_arguments)] // Complex functions need many args
#![allow(clippy::ptr_arg)] // &PathBuf is fine for owned paths

pub mod backend;
#[cfg(feature = "gui")]
pub mod desktop;
pub mod version;

#[cfg(feature = "gui")]
use tauri::{WebviewUrl, WebviewWindowBuilder};

/// Initialize tracing subscriber with profiling support.
/// When built with `--features profiling` AND HIRSEL_PROFILING=1, outputs Chrome Trace Format
/// JSON to `~/.hirsel/profiling/trace-{timestamp}.json` for viewing in Perfetto UI.
pub fn init_process_tracing(process_role: &str) {
    use std::fs;
    use std::path::Path;
    #[cfg(feature = "profiling")]
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::EnvFilter;

    static FILE_GUARD: OnceLock<Mutex<Option<tracing_appender::non_blocking::WorkerGuard>>> =
        OnceLock::new();

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
            EnvFilter::new("hirsel=info,surrealdb=warn,rustls=warn,rustls_platform_verifier=warn,hyper=warn,reqwest=warn")
        })
    }

    let logs_dir = backend::hirsel_dir().join("logs").join(process_role);
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

    let log_prefix = format!("{}.log", process_role);
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
                    backend::hirsel_dir()
                        .join("profiling")
                        .join(chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string())
                });
            fs::create_dir_all(&profiling_dir).expect("Failed to create profiling directory");

            let trace_filename =
                std::env::var("HIRSEL_TRACE_FILENAME").unwrap_or_else(|_| "trace.json".to_string());
            let trace_file = profiling_dir.join(trace_filename);
            eprintln!("[profiling] Writing trace to {}", trace_file.display());
            eprintln!("[hirsel] {} logs -> {}", process_role, logs_dir.display());

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

    eprintln!("[hirsel] {} logs -> {}", process_role, logs_dir.display());
    let _ = tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .try_init();
}

/// Run the GUI (Tauri application)
#[cfg(feature = "gui")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run_desktop() {
    init_process_tracing("desktop");

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // When a second instance tries to launch, focus the existing window
            use tauri::Manager;
            tracing::info!("Second instance attempted with args: {:?}", args);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }));

    builder
        .invoke_handler(desktop::get_handlers())
        .setup(|app| {
            #[cfg(debug_assertions)]
            {
                cleanup_orphaned_dev_processes();
            }

            create_main_window(app.handle())?;
            Ok(())
        })
        .on_window_event(move |window, event| {
            // Scope sessions are backend-owned and may outlive the desktop window.
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
    let (config, _) = backend::config::Config::load()
        .unwrap_or_else(|_| (backend::config::Config::default(), vec![]));
    let mut window_config = app
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or("missing main window config")?;

    let mut initial_url = WebviewUrl::App("index.html".into());
    let backend = config.backend.clone();
    if let Some(url) = backend.url.filter(|value| !value.trim().is_empty()) {
        let api_key = backend.api_key.filter(|value| !value.trim().is_empty());
        {
            let rt = tokio::runtime::Runtime::new()
                .expect("Failed to create tokio runtime for backend bootstrap");
            let should_open = rt.block_on(async {
                let client = reqwest::Client::new();
                let mut request = client.get(format!("{}/health", url.trim_end_matches('/')));
                if let Some(api_key) = api_key.as_deref() {
                    request = request.bearer_auth(api_key);
                }
                match request.send().await {
                    Ok(response) => response.status().is_success(),
                    Err(_) => false,
                }
            });

            if should_open {
                if let Some(api_key) = api_key {
                    let mut bootstrap = format!("{}/connect/bootstrap", url.trim_end_matches('/'))
                        .parse::<tauri::Url>()?;
                    bootstrap
                        .query_pairs_mut()
                        .append_pair("api_key", &api_key)
                        .append_pair("return_to", "/app");
                    initial_url = WebviewUrl::External(bootstrap);
                } else {
                    let app_url =
                        format!("{}/app", url.trim_end_matches('/')).parse::<tauri::Url>()?;
                    initial_url = WebviewUrl::External(app_url);
                }
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
#[cfg(feature = "gui")]
fn cleanup_all_processes() {
    tracing::info!("[GUI] Main window closing, cleaning up GUI processes");
    tracing::info!("[GUI] Cleanup complete");
}

#[cfg(all(feature = "gui", debug_assertions))]
fn cleanup_orphaned_dev_processes() {
    tracing::debug!("[DEV] No legacy helper cleanup needed");
}
