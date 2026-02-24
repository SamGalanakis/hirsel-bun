//! Process management utilities
//!
//! This module provides utilities for managing processes, especially
//! cleanup of process groups when subprocesses exit.

use tracing::info;

/// Clean up the current process group before exiting.
///
/// When a subprocess is spawned with `process_group(0)`, it becomes the
/// process group leader. Calling this function sends SIGTERM to all processes
/// in the current process group, ensuring grandchild processes (like
/// worker helper commands) are properly terminated.
///
/// # Usage
///
/// Call this function at the end of any subprocess entry point that was
/// spawned with `process_group(0)`:
///
/// ```rust,ignore
/// pub fn my_subprocess_entry() -> Result<()> {
///     // ... do work ...
///
///     // Clean up before exiting
///     cleanup_process_group("my-subprocess");
///     Ok(())
/// }
/// ```
///
/// # Safety
///
/// This function uses `libc::kill` to send signals. It sends SIGTERM to
/// the current process group (PID 0), which includes the calling process.
/// Since this is typically called right before the process exits anyway,
/// the signal will either be handled during exit or ignored.
pub fn cleanup_process_group(context: &str) {
    #[cfg(unix)]
    {
        info!("[{}] Cleaning up process group", context);
        unsafe {
            // Send SIGTERM first for graceful shutdown
            libc::kill(0, libc::SIGTERM);
        }

        // Wait for graceful shutdown
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Force kill any remaining processes in the group.
        unsafe {
            libc::kill(0, libc::SIGKILL);
        }
    }

    #[cfg(not(unix))]
    {
        let _ = context; // Suppress unused warning
    }
}
