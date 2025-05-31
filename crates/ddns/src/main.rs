//! Command-line entry point for **ddns**
//!
//! * Parses a single `--config` option (or `DDNS_CONFIG` env var)  
//! * Sets up tracing with a compact formatter  
//! * Boots the core logic defined in `ddns_core`

use anyhow::Result;
use clap::Parser;
use ddns_core::{bootstrap, load_config};
use tracing_subscriber::{filter::EnvFilter, fmt, prelude::*};

/// CLI options
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Path to the config file (optional; environment variables are used if absent)
    #[arg(short, long, env = "DDNS_CONFIG", default_value = "ddns.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("info,tokio_cron_scheduler=warn,axum::rejection=warn")
        }))
        .with(fmt::layer().compact())
        .init();

    let cfg = load_config(&cli.config)?;
    bootstrap(cfg).await
}
