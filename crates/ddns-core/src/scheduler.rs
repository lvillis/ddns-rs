//! Scheduler: periodically detect public IP and refresh DNS records.
//!
//! Since 2025-05-31 the `next_tick` timestamp is written for the dashboard.

use crate::{
    cfg::{AppConfig, ProviderCfg},
    detector::detect_ip,
    status::{Event, EventBus, SharedStatus},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use cron::Schedule;
use ddns_provider::DnsProvider;
use std::{future::Future, pin::Pin, str::FromStr, sync::Arc, time::Duration};
use tokio::{
    sync::{Notify, Semaphore},
    task::JoinHandle,
    time::sleep,
};
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info};

const MAX_RETRY: u8 = 5; // max retries per provider
const BACKOFF_SECS: u64 = 5; // exponential back-off base (seconds)

/*──────── Provider wrapper ────────*/
#[derive(Clone)]
struct ProviderEntry {
    key: String,
    prov: Arc<dyn DnsProvider>,
}

/*──────── entry point ────────*/
pub async fn run_scheduler(cfg: AppConfig, status: SharedStatus, bus: EventBus) -> Result<()> {
    let providers = Arc::new(init_providers(&cfg, &status)?);
    let sem = Arc::new(Semaphore::new(cfg.scheduler.concurrency.unwrap_or(4)));

    /* parse cron expression (if any) */
    let cron_expr = cfg.scheduler.cron.clone();
    let cron_sched = if let Some(ref expr) = cron_expr {
        Some(Arc::new(Schedule::from_str(expr)?))
    } else {
        None
    };

    type BoxFut = Pin<Box<dyn Future<Output = ()> + Send>>;
    let cycle: Arc<dyn Fn() -> BoxFut + Send + Sync> = Arc::new({
        let cfg = cfg.clone();
        let providers = providers.clone();
        let sem = sem.clone();
        let status = status.clone();
        let bus = bus.clone();
        let cron_sched = cron_sched.clone();
        move || {
            let cfg = cfg.clone();
            let providers = providers.clone();
            let sem = sem.clone();
            let status = status.clone();
            let bus = bus.clone();
            let cron_sched = cron_sched.clone();
            Box::pin(async move {
                if let Err(e) =
                    one_cycle(&cfg, &providers, sem, status, bus, cron_sched.as_ref()).await
                {
                    error!("{e:?}");
                }
            })
        }
    });

    /* single-shot or periodic run */
    if let Some(ref expr) = cron_expr {
        let sch = JobScheduler::new().await?;
        let run = cycle.clone();
        sch.add(Job::new_async(expr, move |_, _| (run)())?).await?;
        sch.start().await?;
        info!("cron started: {expr}");
        Notify::new().notified().await; // suspend forever
    } else {
        (cycle)().await;
    }
    Ok(())
}

/*──────── one cycle ────────*/
async fn one_cycle(
    cfg: &AppConfig,
    providers: &[ProviderEntry],
    sem: Arc<Semaphore>,
    status: SharedStatus,
    bus: EventBus,
    cron_sched: Option<&Arc<Schedule>>,
) -> Result<()> {
    let ip = detect_ip(&cfg.detect).await?;
    info!("detected public IP = {ip}");

    /* write status */
    {
        let mut st = status.write();
        st.now = Utc::now();
        st.current_ip = Some(ip.clone());
        st.next_tick = cron_sched.map(|s| s.after(&st.now).next()).flatten();
    }
    let _ = bus.send(Event::Status(status.read().clone()));
    let _ = bus.send(Event::Log(format!("detected IP {ip}")));

    /* update providers concurrently */
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    for entry in providers.iter().cloned() {
        let ip = ip.clone();
        let sem = sem.clone();
        let status = status.clone();
        let bus = bus.clone();
        handles.push(tokio::spawn(async move {
            retry_update(entry, &ip, sem, status, bus).await
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    Ok(())
}

/*──────── update with retry ────────*/
async fn retry_update(
    entry: ProviderEntry,
    ip: &str,
    sem: Arc<Semaphore>,
    status: SharedStatus,
    bus: EventBus,
) {
    let ProviderEntry { key, prov } = entry;
    let mut attempt = 0;
    loop {
        let _permit = sem.acquire().await.unwrap();
        let res = prov
            .upsert_record(prov.zone(), prov.record(), prov.record_type(), ip, 60)
            .await;

        match res {
            Ok(_) => {
                set_stat(&status, &key, Some(Utc::now()), None);
                let _ = bus.send(Event::Status(status.read().clone()));
                let _ = bus.send(Event::Log(format!("{key} OK")));
                break;
            }
            Err(e) if attempt < MAX_RETRY => {
                attempt += 1;
                let wait = BACKOFF_SECS << attempt;
                error!("{key} retry {attempt}/{MAX_RETRY} failed: {e}; waiting {wait}s");
                sleep(Duration::from_secs(wait)).await;
            }
            Err(e) => {
                set_stat(&status, &key, None, Some(e.to_string()));
                let _ = bus.send(Event::Status(status.read().clone()));
                let _ = bus.send(Event::Log(format!("{key} give up: {e}")));
                break;
            }
        }
    }
}

/*──────── Provider initialization ────────*/
fn init_providers(cfg: &AppConfig, status: &SharedStatus) -> Result<Vec<ProviderEntry>> {
    use crate::status::ProviderStat;

    /* ensure keys exist in shared status */
    {
        let mut st = status.write();
        for p in &cfg.provider {
            st.providers
                .entry(display_key(p))
                .or_insert_with(ProviderStat::default);
        }
    }

    let mut v = Vec::new();
    for p in &cfg.provider {
        let prov: Arc<dyn DnsProvider> = match p.kind.to_ascii_lowercase().as_str() {
            #[cfg(feature = "ddns-provider-cloudflare")]
            "cloudflare" => Arc::new(ddns_provider_cloudflare::CfProvider::new(
                &p.zone,
                &p.record,
                &p.record_type,
                p.ttl,
                &p.token,
            )?),

            #[cfg(feature = "ddns-provider-aliyun")]
            "aliyun" => {
                let ak = p
                    .access_key
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("aliyun: access_key missing"))?;
                let sk = p
                    .access_secret
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("aliyun: access_secret missing"))?;
                let region = p.region.as_deref().unwrap_or("cn-hangzhou");
                Arc::new(ddns_provider_aliyun::AliProvider::new(
                    &p.zone,
                    &p.record,
                    &p.record_type,
                    p.ttl,
                    ak,
                    sk,
                    region,
                )?)
            }
            other => anyhow::bail!("unknown provider kind `{other}`"),
        };
        v.push(ProviderEntry {
            key: display_key(p),
            prov,
        });
    }
    Ok(v)
}

fn display_key(p: &ProviderCfg) -> String {
    p.alias.clone().unwrap_or_else(|| p.kind.clone())
}

/*──────── status helper ────────*/
use crate::status::ProviderStat;
fn set_stat(status: &SharedStatus, key: &str, ok: Option<DateTime<Utc>>, err: Option<String>) {
    let mut st = status.write();
    let ent = st
        .providers
        .entry(key.to_owned())
        .or_insert_with(ProviderStat::default);
    if let Some(t) = ok {
        ent.last_ok = Some(t);
        ent.last_err = None;
    }
    if let Some(e) = err {
        ent.last_err = Some(e);
    }
}
