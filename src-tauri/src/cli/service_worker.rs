//! Service worker command - runs as HTTP server for scribe processing.
//!
//! This is spawned by ScribeService when using remote runners, providing
//! HTTP endpoints for processing scribe batches.

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use crate::core::config::Config;
use crate::core::service_worker::ServiceWorkerType;

/// State shared across all handlers
struct ServiceWorkerState {
    /// Type of service worker
    worker_type: ServiceWorkerType,
    /// Last activity timestamp (unix seconds)
    last_activity: AtomicU64,
    /// Idle timeout in seconds
    idle_timeout: u32,
    /// Config for scribe processing
    config: Config,
    /// Agent command for scribe
    agent_command: Vec<String>,
    /// Shutdown signal sender
    shutdown_tx: RwLock<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl ServiceWorkerState {
    fn new(
        worker_type: ServiceWorkerType,
        idle_timeout: u32,
        config: Config,
        agent_command: Vec<String>,
    ) -> Self {
        Self {
            worker_type,
            last_activity: AtomicU64::new(now_secs()),
            idle_timeout,
            config,
            agent_command,
            shutdown_tx: RwLock::new(None),
        }
    }

    fn touch(&self) {
        self.last_activity.store(now_secs(), Ordering::SeqCst);
    }

    fn idle_seconds(&self) -> u64 {
        now_secs().saturating_sub(self.last_activity.load(Ordering::SeqCst))
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// =============================================================================
// API Types
// =============================================================================

/// Health check response
#[derive(Serialize)]
struct HealthResponse {
    status: String,
    worker_type: String,
    idle_seconds: u64,
}

/// Ready message printed to stdout for the manager
#[derive(Serialize)]
struct ReadyMessage {
    status: String,
    port: u16,
}

/// Scribe batch request
#[derive(Deserialize)]
struct ScribeBatchRequest {
    run_name: String,
}

/// Scribe batch response
#[derive(Serialize)]
struct ScribeBatchResponse {
    success: bool,
    batch_id: Option<String>,
    submissions_processed: Option<usize>,
    error: Option<String>,
}

// =============================================================================
// Handlers
// =============================================================================

/// Health check endpoint
async fn health(State(state): State<Arc<ServiceWorkerState>>) -> Json<HealthResponse> {
    state.touch();
    Json(HealthResponse {
        status: "ok".to_string(),
        worker_type: state.worker_type.to_string(),
        idle_seconds: state.idle_seconds(),
    })
}

/// Process scribe batch
async fn scribe_batch(
    State(state): State<Arc<ServiceWorkerState>>,
    Json(req): Json<ScribeBatchRequest>,
) -> Json<ScribeBatchResponse> {
    state.touch();

    if state.worker_type != ServiceWorkerType::Scribe {
        return Json(ScribeBatchResponse {
            success: false,
            batch_id: None,
            submissions_processed: None,
            error: Some("This worker is not configured for scribe".to_string()),
        });
    }

    info!("Processing scribe batch for run: {}", req.run_name);

    // Check run exists
    if !crate::core::config::run_exists(&req.run_name) {
        return Json(ScribeBatchResponse {
            success: false,
            batch_id: None,
            submissions_processed: None,
            error: Some(format!("Run '{}' not found", req.run_name)),
        });
    }

    // Get references to config and agent_command before the async call
    let run_dir = crate::core::config::run_dir(&req.run_name);
    let files = crate::core::Files::new(&run_dir);
    let config = state.config.clone();
    let agent_command = state.agent_command.clone();

    // Spawn scribe processing on a dedicated thread with LocalSet
    // (process_scribe_batch uses spawn_local internally)
    let result: Result<Result<_, _>, _> = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();

        match rt {
            Ok(rt) => rt.block_on(async {
                tokio::task::LocalSet::new()
                    .run_until(async {
                        crate::core::scribe::process_scribe_batch(&files, &config, &agent_command)
                            .await
                    })
                    .await
            }),
            Err(e) => Err(crate::core::scribe::ScribeError::Io(e)),
        }
    })
    .await;

    match result {
        Ok(Ok(batch_result)) => {
            info!(
                "Scribe batch {} processed: {} submissions",
                batch_result.batch_id, batch_result.submissions_processed
            );
            Json(ScribeBatchResponse {
                success: true,
                batch_id: Some(batch_result.batch_id.to_string()),
                submissions_processed: Some(batch_result.submissions_processed),
                error: None,
            })
        }
        Ok(Err(crate::core::scribe::ScribeError::NoPending)) => Json(ScribeBatchResponse {
            success: true,
            batch_id: None,
            submissions_processed: Some(0),
            error: None,
        }),
        Ok(Err(e)) => {
            error!("Scribe batch failed: {}", e);
            Json(ScribeBatchResponse {
                success: false,
                batch_id: None,
                submissions_processed: None,
                error: Some(e.to_string()),
            })
        }
        Err(e) => {
            error!("Scribe task panicked: {}", e);
            Json(ScribeBatchResponse {
                success: false,
                batch_id: None,
                submissions_processed: None,
                error: Some(format!("Task panicked: {}", e)),
            })
        }
    }
}

/// Shutdown endpoint (for graceful termination)
async fn shutdown(State(state): State<Arc<ServiceWorkerState>>) -> StatusCode {
    info!("Shutdown requested");
    if let Some(tx) = state.shutdown_tx.write().await.take() {
        let _ = tx.send(());
    }
    StatusCode::OK
}

// =============================================================================
// Main
// =============================================================================

/// Run the service worker HTTP server
pub async fn execute(
    worker_type_str: &str,
    idle_timeout: u32,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    // Parse worker type (only scribe supported for now)
    let worker_type = match worker_type_str {
        "scribe" => ServiceWorkerType::Scribe,
        _ => {
            return Err(format!(
                "Invalid worker type: {}. Only 'scribe' is supported.",
                worker_type_str
            )
            .into())
        }
    };

    // Load config
    let (config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));

    // Get agent command
    let agent_command = crate::cli::config::get_agent_command();

    // Create shutdown channel
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

    // Create state
    let state = Arc::new(ServiceWorkerState::new(
        worker_type,
        idle_timeout,
        config,
        agent_command,
    ));
    *state.shutdown_tx.write().await = Some(shutdown_tx);

    // Build router
    let app = Router::new()
        .route("/health", get(health))
        .route("/scribe/batch", post(scribe_batch))
        .route("/shutdown", post(shutdown))
        .with_state(Arc::clone(&state));

    // Bind to port (0 = random)
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    let actual_port = listener.local_addr()?.port();

    // Print ready message for the manager
    let ready_msg = ReadyMessage {
        status: "ready".to_string(),
        port: actual_port,
    };
    println!("{}", serde_json::to_string(&ready_msg)?);

    info!(
        "Service worker ({}) listening on port {}",
        worker_type, actual_port
    );

    // Spawn idle timeout checker
    let state_clone = Arc::clone(&state);
    let idle_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let idle_secs = state_clone.idle_seconds();
            debug!(
                "Idle check: {} seconds (timeout: {})",
                idle_secs, state_clone.idle_timeout
            );
            if idle_secs >= state_clone.idle_timeout as u64 {
                info!(
                    "Idle timeout reached ({} >= {}), shutting down",
                    idle_secs, state_clone.idle_timeout
                );
                if let Some(tx) = state_clone.shutdown_tx.write().await.take() {
                    let _ = tx.send(());
                }
                break;
            }
        }
    });

    // Run server until shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
            info!("Shutdown signal received");
        })
        .await?;

    // Clean up
    idle_handle.abort();

    info!("Service worker shutting down");
    Ok(())
}
