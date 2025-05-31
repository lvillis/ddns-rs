//! Runtime status shared between HTTP dashboard and scheduler.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::Serialize;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::broadcast;

#[derive(Clone, Serialize, Default)]
pub struct ProviderStat {
    pub last_ok: Option<DateTime<Utc>>,
    pub last_err: Option<String>,
}

#[derive(Clone, Serialize, Default)]
pub struct AppStatus {
    pub now: DateTime<Utc>,
    pub next_tick: Option<DateTime<Utc>>,
    pub current_ip: Option<String>,
    pub providers: HashMap<String, ProviderStat>,
}

/// Wrapped by `Arc<RwLock<_>>`
pub type SharedStatus = Arc<RwLock<AppStatus>>;

/// Events sent to the front-end via server-sent events
#[derive(Clone, Serialize)]
#[serde(tag = "type", content = "payload")]
pub enum Event {
    #[serde(rename = "status")]
    Status(AppStatus),
    #[serde(rename = "log")]
    Log(String),
}

/// Recommended buffer size is 1024; older events are dropped when full.
pub type EventBus = broadcast::Sender<Event>;
