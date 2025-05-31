use async_trait::async_trait;
use thiserror::Error;

#[derive(Clone, Copy, Debug)]
pub enum RecordType {
    A,
    AAAA,
}

#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api error: {0}")]
    Api(String),
}

#[async_trait]
pub trait DnsProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn zone(&self) -> &str;
    fn record(&self) -> &str;
    fn record_type(&self) -> RecordType;

    async fn upsert_record(
        &self,
        zone: &str,
        name: &str,
        typ: RecordType,
        ip: &str,
        ttl: u32,
    ) -> Result<(), ProviderError>;
}
