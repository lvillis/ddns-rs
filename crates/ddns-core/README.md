<!-- crates/ddns-core/README.md -->

# ddns-core

Runtime backbone of **ddns-rs**.

| Module      | Responsibility                             |
|-------------|--------------------------------------------|
| `scheduler` | Cron loop, IP detection, provider dispatch |
| `http`      | REST + Server-Sent Events via **axum 0.8** |
| `status`    | Thread-safe shared state (`Arc<RwLock>`)   |
| `cfg`       | TOML / ENV / CLI merge with `config` crate |

