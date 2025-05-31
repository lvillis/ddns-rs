//! Public-IP detector set
//!
//! * HTTP       – cross-platform  
//! * Command    – cross-platform  
//! * Interface  – uses `pnet_datalink` on Unix; not supported on Windows

use crate::cfg::DetectCfg;
use anyhow::{Result, anyhow};
use reqwest::Client;
use std::time::Duration;
use tokio::{process::Command, time::timeout};
use tracing::info;

/*──────── interface detector (platform split) ────────*/
#[cfg(unix)]
fn detect_iface(iface: &str) -> Result<String> {
    use pnet_datalink::interfaces;
    use std::net::IpAddr;

    for i in interfaces() {
        if i.name == iface {
            for ipn in i.ips {
                if let IpAddr::V4(v4) = ipn.ip() {
                    return Ok(v4.to_string());
                }
            }
            return Err(anyhow!("interface `{iface}` has no IPv4 address"));
        }
    }
    Err(anyhow!("interface `{iface}` not found"))
}

#[cfg(windows)]
fn detect_iface(_iface: &str) -> Result<String> {
    Err(anyhow!(
        r#"kind = "interface" is not supported on Windows; \
please use `http` or `command` instead"#
    ))
}

/*──────── HTTP detector ────────*/
async fn detect_http(url: &str, to: Option<u64>) -> Result<String> {
    let fut = async {
        Ok::<_, anyhow::Error>(
            Client::new()
                .get(url)
                .send()
                .await?
                .text()
                .await?
                .trim()
                .to_owned(),
        )
    };
    match to {
        Some(ms) => Ok(timeout(Duration::from_millis(ms), fut).await??),
        None => fut.await,
    }
}

/*──────── Command detector ────────*/
async fn detect_cmd(cmd: &str, to: Option<u64>) -> Result<String> {
    let fut = async {
        let out = Command::new("sh").arg("-c").arg(cmd).output().await?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    match to {
        Some(ms) => Ok(timeout(Duration::from_millis(ms), fut).await??),
        None => fut.await,
    }
}

/*──────── orchestrator ────────*/
pub async fn detect_ip(list: &[DetectCfg]) -> Result<String> {
    // default priority is 100 if unspecified
    let mut items = list.to_vec();
    items.sort_by_key(|d| match d {
        DetectCfg::Http { priority, .. }
        | DetectCfg::Interface { priority, .. }
        | DetectCfg::Command { priority, .. } => priority.unwrap_or(100),
    });

    for det in items {
        match det {
            DetectCfg::Http {
                url, timeout: to, ..
            } => {
                if let Ok(ip) = detect_http(&url, to).await {
                    info!("detect/http {url} -> {ip}");
                    return Ok(ip);
                }
            }
            DetectCfg::Interface { iface, .. } => {
                if let Ok(ip) = detect_iface(&iface) {
                    info!("detect/iface {iface} -> {ip}");
                    return Ok(ip);
                }
            }
            DetectCfg::Command {
                cmd, timeout: to, ..
            } => {
                if let Ok(ip) = detect_cmd(&cmd, to).await {
                    info!("detect/cmd `{cmd}` -> {ip}");
                    return Ok(ip);
                }
            }
        }
    }
    Err(anyhow!("all detectors failed"))
}
