use crate::models::ScreenshotResponse;
use crate::{Error, Result};
use image;
use log::{debug, error, info};
use tauri::Runtime;

// Import shared functionality
use crate::desktop::{create_success_response, ScreenshotContext};
use crate::platform::shared::{get_window_title, handle_screenshot_task};
use crate::shared::ScreenshotParams;
use crate::tools::take_screenshot::process_image;

// Linux-specific implementation for taking screenshots using xcap
pub async fn take_screenshot<R: Runtime>(
    params: ScreenshotParams,
    window_context: ScreenshotContext<R>,
) -> Result<ScreenshotResponse> {
    // Clone necessary parameters for use in the closure
    let params_clone = params.clone();
    let window_clone = window_context.window.clone();
    let window_label = params
        .window_label
        .clone()
        .unwrap_or_else(|| "main".to_string());

    // Get application name from params or use a default
    let application_name = params
        .application_name
        .clone()
        .unwrap_or_else(|| "".to_string());

    handle_screenshot_task(move || {
        // Get the window title to help identify the right window
        let window_title = get_window_title(&window_clone)?;

        info!(
            "[TAURI-MCP] Looking for window with title: {} (label: {})",
            window_title, window_label
        );

        // Get all windows using xcap
        let xcap_windows = match xcap::Window::all() {
            Ok(windows) => windows,
            Err(e) => {
                return Err(Error::WindowOperationFailed(format!(
                    "Failed to get window list: {}",
                    e
                )))
            }
        };

        info!(
            "[TAURI-MCP] Found {} windows through xcap",
            xcap_windows.len()
        );

        // Find the target window using optimized search strategy
        if let Some(window) = find_window(&xcap_windows, &window_title, &application_name) {
            // Capture image directly from the window
            let image = match window.capture_image() {
                Ok(img) => img,
                Err(e) => {
                    return Err(Error::WindowOperationFailed(format!(
                        "Failed to capture window image: {}",
                        e
                    )))
                }
            };

            info!(
                "[TAURI-MCP] Successfully captured window image: {}x{}",
                image.width(),
                image.height()
            );

            // Convert to DynamicImage for further processing
            let dynamic_image = image::DynamicImage::ImageRgba8(image);

            // Process the image
            match process_image(dynamic_image, &params_clone) {
                Ok(data_url) => Ok(create_success_response(data_url)),
                Err(e) => Err(e),
            }
        } else {
            // No window found
            Err(Error::WindowOperationFailed(
                "Window not found using any detection method. Please ensure the window is visible and not minimized.".to_string(),
            ))
        }
    })
    .await
}

// Helper function to find the window in the xcap window list
fn find_window(
    xcap_windows: &[xcap::Window],
    window_title: &str,
    application_name: &str,
) -> Option<xcap::Window> {
    let application_name_lower = application_name.to_lowercase();
    let window_title_lower = window_title.to_lowercase();

    debug!(
        "[TAURI-MCP] Searching for window with title: '{}' (case-insensitive)",
        window_title
    );

    // Debug all windows to help with troubleshooting
    debug!("[TAURI-MCP] ============= ALL WINDOWS =============");
    for window in xcap_windows {
        if !window.is_minimized() {
            debug!(
                "[TAURI-MCP] Window: title='{}', app_name='{}'",
                window.title(),
                window.app_name()
            );
        }
    }
    debug!("[TAURI-MCP] ======================================");

    // Step 1: First pass - direct application name match (highest priority)
    if !application_name_lower.is_empty() {
        for window in xcap_windows {
            if window.is_minimized() {
                continue;
            }

            let app_name = window.app_name().to_lowercase();

            // Direct match for application name - highest priority
            if app_name.contains(&application_name_lower) {
                info!(
                    "[TAURI-MCP] Found window by app name: '{}'",
                    window.app_name()
                );
                return Some(window.clone());
            }
        }
    }

    // Step 2: Try matching by window title
    for window in xcap_windows {
        if window.is_minimized() {
            continue;
        }

        let title = window.title().to_lowercase();

        // Match window title
        if title.contains(&window_title_lower) || window_title_lower.contains(&title) {
            info!(
                "[TAURI-MCP] Found window by title: '{}'",
                window.title()
            );
            return Some(window.clone());
        }
    }

    error!(
        "[TAURI-MCP] No matching window found for '{}' or '{}'",
        window_title, application_name
    );
    None
}
