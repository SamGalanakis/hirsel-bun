//! Version and build information for Hirsel
//!
//! This module provides version constants and functions for displaying
//! build information. Values are set at compile time via build.rs.

/// Package version from Cargo.toml
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Git commit SHA (short form, with -dirty suffix if uncommitted changes)
pub const GIT_SHA: &str = env!("HIRSEL_GIT_SHA");

/// Build date (YYYY-MM-DD format)
pub const BUILD_DATE: &str = env!("HIRSEL_BUILD_DATE");

/// Full version string for CLI (compile-time constant)
/// Format: "0.1.0 (abc1234)"
pub const FULL_VERSION: &str =
    concat!(env!("CARGO_PKG_VERSION"), " (", env!("HIRSEL_GIT_SHA"), ")");

/// Get the full version string including git SHA
///
/// Format: "0.1.0 (abc1234)"
pub fn full_version() -> String {
    format!("{} ({})", VERSION, GIT_SHA)
}

/// Get detailed build information
///
/// Format:
/// ```text
/// hirsel 0.1.0 (abc1234)
/// Built: 2024-01-15
/// Features: full-cli, server, tui
/// ```
pub fn build_info() -> String {
    let features = active_features().join(", ");
    format!(
        "hirsel {} ({})\nBuilt: {}\nFeatures: {}",
        VERSION, GIT_SHA, BUILD_DATE, features
    )
}

/// Get list of active features at compile time
pub fn active_features() -> Vec<&'static str> {
    let mut features = Vec::new();

    #[cfg(feature = "gui")]
    features.push("gui");

    #[cfg(feature = "full-cli")]
    features.push("full-cli");

    #[cfg(feature = "server")]
    features.push("server");

    #[cfg(feature = "tui")]
    features.push("tui");

    #[cfg(feature = "worker")]
    features.push("worker");

    if features.is_empty() {
        features.push("minimal");
    }

    features
}

/// Check if this is a worker-only build
pub fn is_worker_build() -> bool {
    #[cfg(all(feature = "worker", not(feature = "full-cli")))]
    return true;

    #[cfg(not(all(feature = "worker", not(feature = "full-cli"))))]
    return false;
}

/// Check if this is a server build
pub fn is_server_build() -> bool {
    #[cfg(feature = "server")]
    return true;

    #[cfg(not(feature = "server"))]
    return false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_not_empty() {
        assert!(!VERSION.is_empty());
        assert!(!GIT_SHA.is_empty());
        assert!(!BUILD_DATE.is_empty());
    }

    #[test]
    fn test_full_version_format() {
        let full = full_version();
        assert!(full.contains(VERSION));
        assert!(full.contains('('));
        assert!(full.contains(')'));
    }

    #[test]
    fn test_active_features() {
        let features = active_features();
        assert!(!features.is_empty());
    }
}
