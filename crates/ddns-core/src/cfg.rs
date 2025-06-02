//! Parse `ddns.toml` into `AppConfig` (intranet-only supported)

use anyhow::Result;
use config::builder::{ConfigBuilder, DefaultState};
use config::{Config, Environment, File};
use serde::{Deserialize, de::DeserializeOwned};
use std::{collections::HashMap, env, path::Path};
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

/// Convert a raw environment value into a `toml::Value`.
///
/// * `"true"` / `"false"`  → `Boolean`  
/// * valid integer         → `Integer` (`i64`)  
/// * valid float           → `Float` (`f64`)  
/// * everything else       → `String`
fn parse_val(raw: &str) -> toml::Value {
    let lower = raw.to_ascii_lowercase();
    if lower == "true" || lower == "false" {
        toml::Value::Boolean(lower == "true")
    } else if let Ok(i) = raw.parse::<i64>() {
        toml::Value::Integer(i)
    } else if let Ok(f) = raw.parse::<f64>() {
        toml::Value::Float(f)
    } else {
        toml::Value::String(raw.to_owned())
    }
}

/// Collect an env-encoded array into `Vec<T>`.
///
/// Example:  
/// `DDNS_PROVIDER_0_KIND=cloudflare` → `provider[0].kind = "cloudflare"`.
///
/// * `section` is the table name (`"provider"` / `"detect"`).  
/// * Returns `Ok(None)` when no matching variables are found.
fn collect_array<T>(section: &str) -> Result<Option<Vec<T>>>
where
    T: DeserializeOwned,
{
    let prefix = format!("DDNS_{}_", section.to_ascii_uppercase());
    let mut buckets: HashMap<usize, toml::Table> = HashMap::new();

    for (k, v) in env::vars() {
        if let Some(rest) = k.strip_prefix(&prefix) {
            // split into "<idx>" and "<FIELD>"
            let mut it = rest.splitn(2, '_');
            let idx = it
                .next()
                .and_then(|s| s.parse::<usize>().ok())
                .ok_or_else(|| anyhow::anyhow!("bad env var: {k}"))?;
            let field = it
                .next()
                .ok_or_else(|| anyhow::anyhow!("bad env var: {k}"))?
                .to_ascii_lowercase();

            buckets.entry(idx).or_default().insert(field, parse_val(&v));
        }
    }

    if buckets.is_empty() {
        return Ok(None);
    }

    // sort by index, then deserialize each table
    let mut out = Vec::new();
    let mut idxs: Vec<_> = buckets.keys().cloned().collect();
    idxs.sort_unstable();
    for i in idxs {
        out.push(buckets.remove(&i).unwrap().try_into()?);
    }
    Ok(Some(out))
}

/// Inject scalar environment variables into a `ConfigBuilder`.
///
/// Keys beginning with `PROVIDER_` / `DETECT_` are skipped because they
/// belong to arrays handled by [`collect_array`].
///
/// # Parameters
/// * `b` – the current builder  
/// * `prefix` – expected environment prefix (`"DDNS_"`).
fn add_scalar_env(
    mut b: ConfigBuilder<DefaultState>,
    prefix: &str,
) -> Result<ConfigBuilder<DefaultState>> {
    let plen = prefix.len();
    for (k, v) in env::vars() {
        if !k.starts_with(prefix) {
            continue;
        }
        let key = &k[plen..]; // remove the prefix, e.g. `HTTP_LISTEN`
        if key.starts_with("PROVIDER_") || key.starts_with("DETECT_") {
            continue; // handled separately
        }
        // HTTP_LISTEN → http.listen
        let path = key.to_ascii_lowercase().replace('_', ".");
        match parse_val(&v) {
            toml::Value::Boolean(bv) => b = b.set_override(path, bv)?,
            toml::Value::Integer(iv) => b = b.set_override(path, iv)?,
            toml::Value::Float(fv) => b = b.set_override(path, fv)?,
            toml::Value::String(sv) => b = b.set_override(path, sv)?,
            _ => unreachable!("only scalar types appear here"),
        }
    }
    Ok(b)
}

/// Load configuration from an optional TOML file **and** environment variables.
///
/// Priority (high → low):
/// 1. Environment arrays (`DDNS_PROVIDER_n_*`, `DDNS_DETECT_n_*`)  
/// 2. Environment scalars (`DDNS_HTTP_LISTEN`, …)  
/// 3. Values in `ddns.toml` (if the file exists)
pub fn load_config(path: &str) -> Result<AppConfig> {
    // 1) start with the optional file
    let mut builder = Config::builder();
    if Path::new(path).exists() {
        builder = builder.add_source(File::with_name(path).required(true));
    } else {
        tracing::info!("config file `{path}` not found; environment-only mode");
    }

    // 2) apply scalar env overrides (non-array)
    builder = add_scalar_env(builder, "DDNS_")?;

    // 3) deserialize into the intermediate Root struct
    let mut root: Root = builder.build()?.try_deserialize()?;

    // 4) replace arrays when env versions exist
    if let Some(v) = collect_array::<DetectCfg>("detect")? {
        root.detect = v;
    }
    if let Some(v) = collect_array::<ProviderCfg>("provider")? {
        root.provider = v;
    }

    // 5) lift into AppConfig
    Ok(AppConfig {
        http: root.http.unwrap_or_default(),
        scheduler: root.scheduler.unwrap_or_default(),
        detect: root.detect,
        provider: root.provider,
    })
}
