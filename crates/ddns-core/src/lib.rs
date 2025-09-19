//! ddns-core - high-level orchestration

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
    let sched_shared = shared.clone();
    let sched_bus = tx.clone();

    tokio::try_join!(
        scheduler::run_scheduler(cfg, sched_shared, sched_bus),
        http::run_http_server(shared, tx, http_cfg)
    )?;

    Ok(())
}

pub use cfg::load_config;
