//! ddns-core – high-level orchestration

pub mod cfg;
pub mod detector;
pub mod error;
mod http;
pub mod scheduler;
pub mod status;

use anyhow::Result;
use cfg::AppConfig;
use status::{Event, SharedStatus};

/// Launches HTTP dashboard and scheduler concurrently.
pub async fn bootstrap(cfg: AppConfig) -> Result<()> {
    let shared: SharedStatus = Default::default();
    let (tx, _rx) = tokio::sync::broadcast::channel::<Event>(1024);

    let http_cfg = cfg.http.clone();

    let http_handle = tokio::spawn(http::run_http_server(shared.clone(), tx.clone(), http_cfg));
    let sched_handle = tokio::spawn(scheduler::run_scheduler(cfg, shared, tx));

    sched_handle.await??;
    http_handle.await??;
    Ok(())
}

pub use cfg::load_config;
