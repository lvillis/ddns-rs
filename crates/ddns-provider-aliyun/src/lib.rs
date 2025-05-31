//! Aliyun DNS provider – production-ready
//!
//! * Supports `A` / `AAAA` record *upsert* (create if absent, update if present).  
//! * Auth via **AccessKey / AccessSecret** – a RAM sub-account with “Read / Write DNS” is enough.  
//! * All API errors are mapped to [`ddns_provider::ProviderError`].  
//! * `zone_id`  is cached via `DescribeDomainInfo`.  
//! * `record_id` is cached via `DescribeSubDomainRecords`.

#![allow(dead_code)]

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use chrono::Utc;
use ddns_provider::{DnsProvider, ProviderError, RecordType};
use hmac::{Hmac, Mac};
use once_cell::sync::OnceCell;
use percent_encoding::{AsciiSet, CONTROLS, percent_encode};
use reqwest::{Client, Response, StatusCode};
use serde_json::Value;
use sha1::Sha1;
use std::collections::BTreeMap;
use tracing::{debug, info};

type HmacSha1 = Hmac<Sha1>;

/// Characters that must be percent-encoded (per Aliyun signing doc, RFC 3986).
const SAFE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

fn encode(v: &str) -> String {
    percent_encode(v.as_bytes(), SAFE).to_string()
}

/*──────── provider struct ────────*/

pub struct AliProvider {
    zone_name: String,
    record_name: String,
    rtype: RecordType,
    ttl: u32,
    ak: String,
    sk: String,
    region: String,
    client: Client,

    zone_id: OnceCell<String>,
    record_id: OnceCell<String>,
}

impl AliProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        zone: &str,
        record: &str,
        record_type: &str,
        ttl: u32,
        access_key: &str,
        access_sec: &str,
        region: &str,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            zone_name: zone.to_owned(),
            record_name: record.to_owned(),
            rtype: if record_type.eq_ignore_ascii_case("AAAA") {
                RecordType::AAAA
            } else {
                RecordType::A
            },
            ttl,
            ak: access_key.to_owned(),
            sk: access_sec.to_owned(),
            region: region.to_owned(),
            client: Client::new(),
            zone_id: OnceCell::new(),
            record_id: OnceCell::new(),
        })
    }

    /*──────── signed request helper ────────*/

    async fn call(&self, mut params: BTreeMap<String, String>) -> Result<Value, ProviderError> {
        // common params
        params.insert("Format".into(), "JSON".into());
        params.insert("Version".into(), "2015-01-09".into());
        params.insert("AccessKeyId".into(), self.ak.clone());
        params.insert(
            "Timestamp".into(),
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        );
        params.insert("SignatureMethod".into(), "HMAC-SHA1".into());
        params.insert("SignatureVersion".into(), "1.0".into());
        params.insert("SignatureNonce".into(), uuid::Uuid::new_v4().to_string());

        // canonicalize
        let canon_query = params
            .iter()
            .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let string_to_sign = format!("GET&%2F&{}", encode(&canon_query));

        // HMAC-SHA1
        let mut mac =
            HmacSha1::new_from_slice(format!("{}&", self.sk).as_bytes()).expect("HMAC key length");
        mac.update(string_to_sign.as_bytes());
        let sign = B64.encode(mac.finalize().into_bytes());

        // final URL
        let url = format!(
            "https://aliyun.aliyuncs.com/?Signature={}&{}",
            encode(&sign),
            canon_query
        );

        let resp: Response = self.client.get(url).send().await?;
        let status = resp.status();
        let v: Value = resp.json().await?;

        if status == StatusCode::OK {
            Ok(v)
        } else {
            Err(ProviderError::Api(
                v["Message"].as_str().unwrap_or("Aliyun error").to_string(),
            ))
        }
    }

    /*──────── zone / record helpers ────────*/

    async fn ensure_zone_id(&self) -> Result<&str, ProviderError> {
        if let Some(id) = self.zone_id.get() {
            return Ok(id);
        }

        let mut p = BTreeMap::new();
        p.insert("Action".into(), "DescribeDomainInfo".into());
        p.insert("DomainName".into(), self.zone_name.clone());
        let v = self.call(p).await?;
        let id = v["DomainId"]
            .as_str()
            .ok_or_else(|| ProviderError::Api("zone not found".into()))?;
        let _ = self.zone_id.set(id.to_owned());
        Ok(self.zone_id.get().expect("zone_id set"))
    }

    async fn ensure_record_id(&self) -> Result<Option<&str>, ProviderError> {
        if let Some(id) = self.record_id.get() {
            return Ok(Some(id));
        }
        let mut p = BTreeMap::new();
        p.insert("Action".into(), "DescribeSubDomainRecords".into());
        p.insert(
            "SubDomain".into(),
            format!("{}.{}", self.record_name, self.zone_name),
        );
        let v = self.call(p).await?;
        if let Some(id) = v["DomainRecords"]["Record"][0]["RecordId"].as_str() {
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

    async fn add_record(&self, ip: &str) -> Result<(), ProviderError> {
        let mut p = BTreeMap::new();
        p.insert("Action".into(), "AddDomainRecord".into());
        p.insert("DomainName".into(), self.zone_name.clone());
        p.insert("RR".into(), self.record_name.clone());
        p.insert("Type".into(), self.rtype_str().into());
        p.insert("Value".into(), ip.into());
        p.insert("TTL".into(), self.ttl.to_string());

        let v = self.call(p).await?;
        let id = v["RecordId"]
            .as_str()
            .ok_or_else(|| ProviderError::Api("add: missing RecordId".into()))?;
        let _ = self.record_id.set(id.to_owned());
        info!("Aliyun created record id={id}");
        Ok(())
    }

    async fn update_record(&self, rid: &str, ip: &str) -> Result<(), ProviderError> {
        let mut p = BTreeMap::new();
        p.insert("Action".into(), "UpdateDomainRecord".into());
        p.insert("RecordId".into(), rid.to_owned());
        p.insert("RR".into(), self.record_name.clone());
        p.insert("Type".into(), self.rtype_str().into());
        p.insert("Value".into(), ip.into());
        p.insert("TTL".into(), self.ttl.to_string());
        self.call(p).await?;
        info!("Aliyun updated record id={rid}");
        Ok(())
    }
}

/*──────── DnsProvider impl ────────*/

#[async_trait]
impl DnsProvider for AliProvider {
    fn name(&self) -> &'static str {
        "Aliyun"
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
        let rid_opt = self.ensure_record_id().await?;
        match rid_opt {
            Some(rid) => self.update_record(rid, ip).await,
            None => self.add_record(ip).await,
        }?;
        debug!(
            "Aliyun upsert {}.{} -> {}",
            self.record_name, self.zone_name, ip
        );
        Ok(())
    }
}

/*──────── optional live-test (ignored by default) ────────*/
#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_upsert() {
        let ak = env::var("ALI_KEY").unwrap();
        let sk = env::var("ALI_SEC").unwrap();

        let ali =
            AliProvider::new("example.com", "test-ddns", "A", 60, &ak, &sk, "cn-hangzhou").unwrap();

        ali.upsert_record("example.com", "test-ddns", RecordType::A, "1.1.1.1", 60)
            .await
            .unwrap();
    }
}
