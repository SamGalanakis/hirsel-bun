//! Thin Tauri desktop host for Hirsel.

#![allow(clippy::should_implement_trait)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::ptr_arg)]

#[cfg(feature = "gui")]
pub mod desktop;

#[cfg(feature = "gui")]
use tauri::{WebviewUrl, WebviewWindowBuilder};

/// Run the GUI (Tauri application)
#[cfg(feature = "gui")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run_desktop() {
    hirsel_core::init_process_tracing("desktop");

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
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
    let (config, _) = hirsel_core::backend::config::Config::load()
        .unwrap_or_else(|_| (hirsel_core::backend::config::Config::default(), vec![]));
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
        if let Some(api_key) = api_key {
            let mut bootstrap =
                format!("{}/connect/bootstrap", url.trim_end_matches('/')).parse::<tauri::Url>()?;
            bootstrap
                .query_pairs_mut()
                .append_pair("api_key", &api_key)
                .append_pair("return_to", "/app");
            initial_url = WebviewUrl::External(bootstrap);
        } else {
            let app_url = format!("{}/app", url.trim_end_matches('/')).parse::<tauri::Url>()?;
            initial_url = WebviewUrl::External(app_url);
        }
    }

    window_config.create = true;
    window_config.url = initial_url;
    let window = WebviewWindowBuilder::from_config(app, &window_config)?.build()?;
    let _ = window.set_background_color(Some(tauri::window::Color(26, 26, 26, 255)));
    Ok(())
}

#[cfg(feature = "gui")]
fn cleanup_all_processes() {
    tracing::info!("[GUI] Main window closing, cleaning up GUI processes");
    tracing::info!("[GUI] Cleanup complete");
}

#[cfg(all(feature = "gui", debug_assertions))]
fn cleanup_orphaned_dev_processes() {
    tracing::debug!("[DEV] No legacy helper cleanup needed");
}
