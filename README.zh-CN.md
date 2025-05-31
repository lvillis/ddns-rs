<!-- ─── 语言切换 & 目录（右上角） ─────────────────────────────── -->
<div align="right">

<a href="README.md">🇺🇸 English</a> ·
<a aria-disabled="true" style="color:#999;text-decoration:none;">🇨🇳 中文</a>

<br/>
目录 ↗️
</div>

<h1 align="center"><code>ddns-rs</code></h1>

<p align="center">
  🌐 <strong>Rust 动态 DNS 一体化工具</strong> — 自动侦测公网 IP，并同时更新 <em>多家</em> DNS 解析记录；内置仪表盘，零额外运行依赖。
</p>

<div align="center">

[![Crates.io](https://img.shields.io/crates/v/ddns.svg)](https://crates.io/crates/ddns)
[![Repo Size](https://img.shields.io/github/repo-size/lvillis/ddns-rs?color=328657)](https://github.com/lvillis/ddns-rs)
[![CI](https://github.com/lvillis/ddns-rs/actions/workflows/ci.yaml/badge.svg)](https://github.com/lvillis/ddns-rs/actions)
[![Docker Pulls](https://img.shields.io/docker/pulls/lvillis/ddns-rs)](https://hub.docker.com/r/lvillis/ddns-rs)
[![Image Size](https://img.shields.io/docker/image-size/lvillis/ddns-rs/latest?style=flat-square)](https://hub.docker.com/r/lvillis/ddns-rs)
[![MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

</div>

---

## ✨ 功能亮点

| 功能                          | 说明                                                                 |
|-------------------------------|----------------------------------------------------------------------|
| **多云厂商变更 (upsert)**     | 内置 Cloudflare & Aliyun 驱动；亦可通过 feature flag 添加自定义驱动 |
| **可插拔 IP 探测器**          | HTTP · 本机网卡 · 自定义 Shell，支持优先级链                         |
| **基于 Cron 的调度器**        | 6 字段标准 Cron（秒级）+ 并发控制 + 退避重试                         |
| **自托管仪表盘**              | Tailwind + Alpine，自动深浅主题；支持 Cookie 和 Bearer 认证          |
| **零运行依赖**                | 静态单文件可执行或多架构 Docker 镜像（< 10 MB）                      |
| **环境变量覆盖**              | 任何 TOML 键都可用 `DDNS__SECTION__KEY` 覆盖                        |

## 🖼 架构示意

```mermaid
graph TD
%% ── Client Layer ───────────────────────
    subgraph "客户端"
        Browser["浏览器<br/><sub>Dashboard UI</sub>"]
        ApiTool["REST 客户端 / cURL"]
    end
    class Browser,ApiTool client;

%% ── Core Daemon ────────────────────────
    subgraph "ddns-rs 守护进程"
        HTTP["HTTP 服务<br/><sub>axum 0.8</sub>"]
        Scheduler["任务调度<br/><sub>cron + 回退</sub>"]
        Detector["IP 探测<br/><sub>HTTP • NIC • Shell</sub>"]
        Status["共享状态<br/><sub>Arc&lt;RwLock&gt;</sub>"]
    end
    class HTTP,Scheduler,Detector,Status daemon;

%% ── Provider Layer ─────────────────────
    subgraph "DNS 服务商"
        Cloudflare
        Aliyun
        Custom["自定义驱动"]
    end
    class Cloudflare,Aliyun,Custom provider;

%% ── Interactions ───────────────────────
    Browser  -- "SSE / REST" --> HTTP
    ApiTool  -- REST         --> HTTP

    HTTP     --> Status
    Scheduler --> Detector
    Detector  --> Scheduler
    Scheduler --> Status

    Scheduler --> Cloudflare
    Scheduler --> Aliyun
    Scheduler --> Custom

%% ── Styling ───────────────────────────
    classDef client    fill:#e3f2fd,stroke:#1976d2,stroke-width:1px;
    classDef daemon    fill:#e8f5e9,stroke:#388e3c,stroke-width:1px;
    classDef provider  fill:#fff8e1,stroke:#f57f17,stroke-width:1px;
```

## 🐳 Docker

```shell
docker run --rm \
  -v $PWD/ddns.toml:/opt/app/ddns.toml \
  -p 8080:8080 \
  -e DDNS__HTTP__JWT_SECRET=$JWT_SECRET \
  lvillis/ddns-rs
```