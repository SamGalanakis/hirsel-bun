//! Helper functions shared across GUI commands

// Re-export shared helper functions from core::api_types
pub use crate::core::api_types::{
    calculate_duration_minutes, convert_status, is_completed_status, parse_elapsed_minutes,
    parse_timestamp,
};
