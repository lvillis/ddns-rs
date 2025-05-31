<!-- crates/ddns-provider-cloudflare/README.md -->

# ddns-provider-cloudflare

Cloudflare DNS driver for **ddns-rs**.

* Handles `A` / `AAAA` records with automatic **create or update**
* Auth via **API Token** (`Zone-Read` + `DNS-Edit`)
* Local cache of `zone_id` and `record_id` for speed