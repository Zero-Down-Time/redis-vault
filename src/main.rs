#![forbid(unsafe_code)]

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

mod backup;
mod config;
mod logging;
mod metrics;
mod storage;

use backup::BackupManager;
use config::load_config;
use logging::init_logging;
use metrics::Metrics;

// CLI Arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,

    /// Run once and exit (for testing)
    #[arg(long)]
    once: bool,
}

fn spawn_metrics_server(
    metrics: Arc<RwLock<metrics::Metrics>>,
    port: u16,
    listen_address: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(e) = metrics::start_metrics_server(metrics, port, listen_address).await {
            error!("Metrics server failed: {}", e);
        }
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse arguments
    let args = Args::parse();

    // Load configuration
    let config = load_config(&args.config)?;

    // Initialize logging using custom config
    init_logging(&config.logging.level, &config.logging.format);

    debug!("Config: {:?}", config);

    // Initialize metrics
    let metrics = Arc::new(RwLock::new(Metrics::new()?));

    // Start metrics server if enabled
    let metrics_handle = if config.metrics.enabled {
        debug!("Metrics initialized");
        Some(spawn_metrics_server(
            metrics.clone(),
            config.metrics.port,
            config.metrics.listen_address.clone(),
        ))
    } else {
        info!("Metrics server disabled");
        None
    };

    // Create and run backup manager
    let mut manager = BackupManager::new(config, metrics).await?;

    // Run backup manager
    let backup_result = manager.run(args.once).await;

    // If we started a metrics server, we should shut it down gracefully
    if let Some(handle) = metrics_handle {
        handle.abort();
    }

    backup_result
}
