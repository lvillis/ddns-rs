<!-- crates/ddns-provider/README.md -->

# ddns-provider (traits)

Thin abstraction for pluggable DNS providers.

```rust
#[async_trait::async_trait]
pub trait DnsProvider {
    async fn upsert_record(
        &self,
        zone: &str,
        name: &str,
        typ: RecordType,
        ip: &str,
        ttl: u32,
    ) -> Result<(), ProviderError>;
}
```