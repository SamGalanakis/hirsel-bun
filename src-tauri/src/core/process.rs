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
/// `node hirsel __acp-bridge` spawned by `claude`) are properly terminated.
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

        // Force kill any remaining processes in the group
        // (Node.js processes like hirsel __acp-bridge may ignore SIGTERM)
        unsafe {
            libc::kill(0, libc::SIGKILL);
        }
    }

    #[cfg(not(unix))]
    {
        let _ = context; // Suppress unused warning
    }
}

/// Kill a specific process group by PID.
///
/// This is useful when you have the PID of a process group leader and want
/// to kill all processes in that group.
///
/// # Arguments
///
/// * `pid` - The PID of the process group leader
/// * `force` - If true, sends SIGKILL after SIGTERM (always, since children may survive)
pub fn kill_process_group(pid: u32, force: bool) {
    #[cfg(unix)]
    {
        unsafe {
            // Send SIGTERM to the process group
            libc::kill(-(pid as i32), libc::SIGTERM);
        }

        if force {
            // Brief wait for graceful shutdown
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Always send SIGKILL - the leader may die quickly but children
            // (like Node.js hirsel __acp-bridge) may ignore SIGTERM
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (pid, force); // Suppress unused warnings
    }
}

/// Check if a process is still running.
#[cfg(unix)]
pub fn is_pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
pub fn is_pid_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_pid_alive_self() {
        // Our own PID should be alive
        let pid = std::process::id();
        assert!(is_pid_alive(pid));
    }

    #[test]
    fn test_is_pid_alive_invalid() {
        // PID 0 is special (kernel), but checking shouldn't panic
        // A very large PID is unlikely to exist
        assert!(!is_pid_alive(999999999));
    }
}
