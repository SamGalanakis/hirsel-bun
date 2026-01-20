//! Worker scaling configuration.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use super::ConfigError;

/// Regex pattern for parsing worker scale
static WORKER_SCALE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)$").expect("invalid regex"));

/// Worker scaling configuration.
///
/// Workers is just a max count - always starts with 1 and autoscales up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerScale {
    pub max: u32,
}

impl WorkerScale {
    /// Parse worker scale from string - just the max worker count.
    /// - "4" -> autoscale up to 4 workers
    pub fn parse(value: &str) -> Result<Self, ConfigError> {
        let value = value.trim();

        // Simple number = max workers
        if let Some(caps) = WORKER_SCALE_RE.captures(value) {
            let max: u32 = caps[1].parse().unwrap();
            if max < 1 {
                return Err(ConfigError::WorkerCountTooLow);
            }
            return Ok(Self { max });
        }

        Err(ConfigError::InvalidWorkerScale {
            value: value.to_string(),
        })
    }

    /// Number of workers to start with - always 1, we autoscale from there
    pub fn initial_count(&self) -> u32 {
        1
    }

    /// Check if we can add more workers
    pub fn can_scale_up(&self, current: u32) -> bool {
        current < self.max
    }
}

impl Default for WorkerScale {
    fn default() -> Self {
        Self { max: 1 }
    }
}

impl std::fmt::Display for WorkerScale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_scale_simple() {
        let scale = WorkerScale::parse("3").unwrap();
        assert_eq!(scale.max, 3);
        assert_eq!(scale.initial_count(), 1);
        assert_eq!(scale.to_string(), "3");
    }

    #[test]
    fn test_worker_scale_invalid() {
        assert!(WorkerScale::parse("0").is_err());
        assert!(WorkerScale::parse("abc").is_err());
        assert!(WorkerScale::parse("1-5").is_err()); // Legacy format not supported
        assert!(WorkerScale::parse("2+").is_err()); // Legacy format not supported
    }

    #[test]
    fn test_worker_scale_can_scale_up() {
        let scale = WorkerScale::parse("5").unwrap();
        assert!(scale.can_scale_up(3));
        assert!(scale.can_scale_up(4));
        assert!(!scale.can_scale_up(5));
        assert!(!scale.can_scale_up(6));
    }
}
