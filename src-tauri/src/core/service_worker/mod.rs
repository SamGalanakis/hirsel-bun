//! Service worker management for long-running background services.
//!
//! This module provides the ScribeService for processing documentation batches.
//! The service handles local vs remote execution internally.
//!
//! ## Usage
//!
//! ```ignore
//! let service = ScribeService::with_config(config);
//! service.process_batch("my-run").await?;
//! ```
//!
//! ## Configuration
//!
//! ```toml
//! [service_workers]
//! runner = "fly"  # Default runner for all service workers
//!
//! [service_workers.scribe]
//! runner = "local"  # Override for scribe
//! idle_timeout_seconds = 300
//! ```

mod scribe;
mod types;

pub use scribe::{create_scribe_service, ScribeService};
pub use types::{ServiceWorkerError, ServiceWorkerHandle, ServiceWorkerResult, ServiceWorkerType};
