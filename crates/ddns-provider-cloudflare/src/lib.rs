//! Cloudflare DNS provider – production-ready
//!
//! * Supports `A` / `AAAA` record *upsert* (create or update).  
//! * Auth via **API Token** (recommended) – needs `Zone:Read` and `DNS:Edit`.  
//! * `zone_id`  and `record_id` are cached locally to reduce API calls.  
//! * All business errors are mapped to [`ddns_provider::ProviderError`].

use async_trait::async_trait;
use ddns_provider::{DnsProvider, ProviderError, RecordType};
use once_cell::sync::OnceCell;
use reqwest::{
    Client, Response, StatusCode,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue, USER_AGENT},
};
use serde_json::{Value, json};
use tracing::{debug, info};

const API_ROOT: &str = "https://api.cloudflare.com/client/v4";

/*──────── provider struct ────────*/

pub struct CfProvider {
    zone_name: String,
    record_name: String,
    rtype: RecordType,
    ttl: u32,
    client: Client,

    zone_id: OnceCell<String>,
    record_id: OnceCell<String>,
}

impl CfProvider {
    pub fn new(
        zone: &str,
        record: &str,
        rtype: &str,
        ttl: u32,
        token: &str,
    ) -> anyhow::Result<Self> {
        let mut hdr = HeaderMap::new();
        hdr.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))?,
        );
        hdr.insert(USER_AGENT, HeaderValue::from_static("ddns-rs (+github)"));
        hdr.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        Ok(Self {
            zone_name: zone.to_owned(),
            record_name: record.to_owned(),
            rtype: if rtype.eq_ignore_ascii_case("AAAA") {
                RecordType::AAAA
            } else {
                RecordType::A
            },
            ttl,
            client: Client::builder().default_headers(hdr).build()?,
            zone_id: OnceCell::new(),
            record_id: OnceCell::new(),
        })
    }

    /*──────── tiny HTTP wrapper ────────*/

    async fn get(&self, path: &str) -> Result<Value, ProviderError> {
        self.check(self.client.get(format!("{API_ROOT}{path}")).send().await?)
            .await
    }
    async fn post(&self, path: &str, body: Value) -> Result<Value, ProviderError> {
        self.check(
            self.client
                .post(format!("{API_ROOT}{path}"))
                .json(&body)
                .send()
                .await?,
        )
        .await
    }
    async fn put(&self, path: &str, body: Value) -> Result<Value, ProviderError> {
        self.check(
            self.client
                .put(format!("{API_ROOT}{path}"))
                .json(&body)
                .send()
                .await?,
        )
        .await
    }

    async fn check(&self, resp: Response) -> Result<Value, ProviderError> {
        let status = resp.status();
        let v: Value = resp.json().await?;
        if status == StatusCode::OK && v["success"].as_bool().unwrap_or(false) {
            Ok(v)
        } else {
            let msg = v["errors"]
                .get(0)
                .and_then(|e| e["message"].as_str())
                .unwrap_or("unknown error");
            Err(ProviderError::Api(msg.to_owned()))
        }
    }

    /*──────── zone / record helpers ────────*/

    async fn ensure_zone_id(&self) -> Result<&str, ProviderError> {
        if let Some(id) = self.zone_id.get() {
            return Ok(id);
        }
        let v = self.get(&format!("/zones?name={}", self.zone_name)).await?;
        let id = v["result"]
            .get(0)
            .and_then(|r| r["id"].as_str())
            .ok_or_else(|| ProviderError::Api("zone not found".into()))?;
        let _ = self.zone_id.set(id.to_owned());
        Ok(self.zone_id.get().expect("zone_id set"))
    }

    async fn ensure_record_id(&self) -> Result<Option<&str>, ProviderError> {
        if let Some(id) = self.record_id.get() {
            return Ok(Some(id));
        }
        let zid = self.ensure_zone_id().await?;
        let full = format!("{}.{}", self.record_name, self.zone_name);
        let v = self
            .get(&format!(
                "/zones/{zid}/dns_records?type={}&name={full}",
                self.rtype_str()
            ))
            .await?;
        if let Some(id) = v["result"].get(0).and_then(|r| r["id"].as_str()) {
            let _ = self.record_id.set(id.to_owned());
            Ok(Some(self.record_id.get().unwrap()))
        } else {
            Ok(None)
        }
    }

    fn rtype_str(&self) -> &'static str {
        match self.rtype {
            RecordType::A => "A",
            RecordType::AAAA => "AAAA",
        }
    }

    /*──────── create / update helpers ────────*/

    async fn create_record(&self, zid: &str, content: &str) -> Result<(), ProviderError> {
        let body = json!({
            "type":    self.rtype_str(),
            "name":    self.record_name,
            "content": content,
            "ttl":     self.ttl,
            "proxied": false
        });
        let v = self
            .post(&format!("/zones/{zid}/dns_records"), body)
            .await?;
        let id = v["result"]["id"]
            .as_str()
            .ok_or_else(|| ProviderError::Api("create: missing id".into()))?;
        let _ = self.record_id.set(id.to_owned());
        info!("Cloudflare created record id={id}");
        Ok(())
    }

    async fn update_record(
        &self,
        zid: &str,
        rid: &str,
        content: &str,
    ) -> Result<(), ProviderError> {
        let body = json!({
            "type":    self.rtype_str(),
            "name":    self.record_name,
            "content": content,
            "ttl":     self.ttl,
            "proxied": false
        });
        self.put(&format!("/zones/{zid}/dns_records/{rid}"), body)
            .await?;
        info!("Cloudflare updated record id={rid}");
        Ok(())
    }
}

/*──────── DnsProvider impl ────────*/

#[async_trait]
impl DnsProvider for CfProvider {
    fn name(&self) -> &'static str {
        "Cloudflare"
    }
    fn zone(&self) -> &str {
        &self.zone_name
    }
    fn record(&self) -> &str {
        &self.record_name
    }
    fn record_type(&self) -> RecordType {
        self.rtype
    }

    async fn upsert_record(
        &self,
        _zone: &str,
        _name: &str,
        _typ: RecordType,
        ip: &str,
        _ttl: u32,
    ) -> Result<(), ProviderError> {
        let zid = self.ensure_zone_id().await?;
        match self.ensure_record_id().await? {
            Some(rid) => self.update_record(zid, rid, ip).await,
            None => self.create_record(zid, ip).await,
        }?;
        debug!(
            "Cloudflare upsert {}.{} -> {}",
            self.record_name, self.zone_name, ip
        );
        Ok(())
    }
}

/*──────── optional integration test (ignored) ────────*/
#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_upsert() {
        let token = env::var("CF_TOKEN").expect("CF_TOKEN not set");
        let cf = CfProvider::new("example.com", "test-ddns", "A", 60, &token).unwrap();
        cf.upsert_record("example.com", "test-ddns", RecordType::A, "1.1.1.1", 60)
            .await
            .unwrap();
    }
}
