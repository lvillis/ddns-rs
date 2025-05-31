<!-- crates/ddns/README.md -->

# ddns (binary)

`ddns` is the zero-dependency **Dynamic-DNS daemon** shipping with:

- **Cron scheduler** with second precision
- Built-in web dashboard (`Tailwind + Alpine`)
- Multi-provider support via feature flags
- Single static executable or tiny Docker image

```bash
# Run with the default providers
cargo install ddns --features "ddns-provider-aliyun ddns-provider-cloudflare"
ddns -c ddns.toml
