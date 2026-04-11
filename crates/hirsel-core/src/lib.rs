//! Hirsel - Herd your AI coding agents

#![allow(clippy::should_implement_trait)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::ptr_arg)]

pub mod backend;
pub mod version;

/// Initialize tracing subscriber with a central log file and optional profiling.
pub fn init_process_tracing(process_role: &str) {
    use std::fs;
    #[cfg(feature = "profiling")]
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::EnvFilter;

    static FILE_GUARD: OnceLock<Mutex<Option<tracing_appender::non_blocking::WorkerGuard>>> =
        OnceLock::new();

    fn default_filter() -> EnvFilter {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("hirsel=info,surrealdb=warn,rustls=warn,rustls_platform_verifier=warn,hyper=warn,reqwest=warn")
        })
    }

    let logs_dir = backend::hirsel_dir().join("logs");
    if let Err(error) = fs::create_dir_all(&logs_dir) {
        eprintln!(
            "[hirsel] failed to create log directory {}: {}",
            logs_dir.display(),
            error
        );
    }

    let max_bytes: u64 = std::env::var("HIRSEL_LOG_MAX_MB")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(20)
        * 1024
        * 1024;
    let keep_rotated: usize = std::env::var("HIRSEL_LOG_KEEP_FILES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(5);

    let log_path = logs_dir.join("hirsel.log");
    let should_rotate = log_path
        .metadata()
        .map(|m| m.len() >= max_bytes)
        .unwrap_or(false);
    if should_rotate {
        for i in (1..keep_rotated).rev() {
            let from = logs_dir.join(format!("hirsel.log.{}", i));
            let to = logs_dir.join(format!("hirsel.log.{}", i + 1));
            let _ = fs::rename(&from, &to);
        }
        let _ = fs::rename(&log_path, logs_dir.join("hirsel.log.1"));
        let oldest = logs_dir.join(format!("hirsel.log.{}", keep_rotated + 1));
        let _ = fs::remove_file(oldest);
    }

    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("Failed to open hirsel.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(log_file);
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
            eprintln!("[hirsel] {} logs -> {}", process_role, log_path.display());

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

    eprintln!("[hirsel] {} logs -> {}", process_role, log_path.display());
    let _ = tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .try_init();
}
