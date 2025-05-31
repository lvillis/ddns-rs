//! Parse `ddns.toml` into `AppConfig` (intranet-only supported)

use anyhow::Result;
use config::{Config, Environment, File};
use serde::Deserialize;
use validator::Validate;

/*──────── Provider ────────*/
#[derive(Debug, Clone, Deserialize, Validate)]
pub struct ProviderCfg {
    #[validate(length(min = 1))]
    pub kind: String,
    #[validate(length(min = 1))]
    pub zone: String,
    #[validate(length(min = 1))]
    pub record: String,

    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub record_type: String,
    #[serde(default)]
    pub ttl: u32,

    // cloudflare
    #[serde(default)]
    pub token: String,
    // aliyun
    #[serde(default)]
    pub access_key: Option<String>,
    #[serde(default)]
    pub access_secret: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
}

/*──────── Detect ────────*/
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DetectCfg {
    Http {
        url: String,
        /// timeout in milliseconds
        #[serde(default)]
        timeout: Option<u64>,
        #[serde(default)]
        priority: Option<u32>,
    },
    Interface {
        /// network interface name, e.g. `eth0`
        iface: String,
        #[serde(default)]
        priority: Option<u32>,
    },
    Command {
        /// arbitrary shell command that outputs the public IP
        cmd: String,
        /// timeout in milliseconds
        #[serde(default)]
        timeout: Option<u64>,
        #[serde(default)]
        priority: Option<u32>,
    },
}

/*──────── Scheduler ────────*/
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SchedulerCfg {
    /// cron expression; if `None`, runs exactly once
    pub cron: Option<String>,
    /// max concurrent provider updates
    pub concurrency: Option<usize>,
}

/*──────── HTTP ────────*/
#[derive(Debug, Clone, Deserialize)]
pub struct AuthCfg {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpCfg {
    /// listening address
    #[serde(default = "default_listen")]
    pub listen: String,
    /// restrict to private/loopback networks; default `true`
    #[serde(default = "default_true")]
    pub intranet_only: bool,
    /// JWT secret (≥ 256 bit); use env var in production
    #[serde(default = "default_secret")]
    pub jwt_secret: String,
    /// JWT life-time in seconds
    #[serde(default = "default_ttl")]
    pub token_ttl_sec: u64,
    /// enable cookie login; `None` → anonymous access
    #[serde(default)]
    pub auth: Option<AuthCfg>,
}
fn default_listen() -> String {
    "0.0.0.0:8080".to_string()
}
fn default_true() -> bool {
    true
}
fn default_secret() -> String {
    "dev_only_change_me".into()
}
fn default_ttl() -> u64 {
    24 * 60 * 60
}

impl Default for HttpCfg {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            intranet_only: true,
            jwt_secret: default_secret(),
            token_ttl_sec: default_ttl(),
            auth: None,
        }
    }
}

/*──────── Root & AppConfig ────────*/
#[derive(Debug, Deserialize)]
struct Root {
    #[serde(default)]
    http: Option<HttpCfg>,
    #[serde(default)]
    scheduler: Option<SchedulerCfg>,
    #[serde(default)]
    detect: Vec<DetectCfg>,
    provider: Vec<ProviderCfg>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub http: HttpCfg,
    pub scheduler: SchedulerCfg,
    pub detect: Vec<DetectCfg>,
    pub provider: Vec<ProviderCfg>,
}

/*──────── entry point ────────*/
pub fn load_config(path: &str) -> Result<AppConfig> {
    use std::path::Path;

    let mut builder =
        Config::builder().add_source(Environment::with_prefix("DDNS").separator("__"));

    if Path::new(path).exists() {
        builder = builder.add_source(File::with_name(path).required(true));
    } else {
        tracing::info!("config file `{path}` not found; environment-only mode");
    }

    let root: Root = builder.build()?.try_deserialize()?;

    Ok(AppConfig {
        http: root.http.unwrap_or_default(),
        scheduler: root.scheduler.unwrap_or_default(),
        detect: root.detect,
        provider: root.provider,
    })
}
