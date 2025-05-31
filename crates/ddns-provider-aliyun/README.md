<!-- crates/ddns-provider-aliyun/README.md -->

# ddns-provider-aliyun

Aliyun (Alibaba Cloud) DNS driver for **ddns-rs**.

* Supports `A` / `AAAA` record **upsert** (create-or-update).
* Caches `zone_id` & `record_id` to minimise API calls.
* Auth via **AccessKey / AccessSecret** (RAM sub-account is enough).
