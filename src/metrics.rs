use anyhow::Result;
use prometheus::{Encoder, Gauge, Histogram, HistogramOpts, IntCounter, Registry, TextEncoder};
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::Filter;

#[derive(Clone)]
pub struct Metrics {
    registry: Arc<Registry>,

    // Backup operation counters
    pub backups_total: IntCounter,
    pub backups_successful: IntCounter,
    pub backups_failed: IntCounter,

    // Backup operation details
    pub backup_size_bytes: Histogram,
    pub backup_duration_seconds: Histogram,
    pub last_backup_timestamp: Gauge,

    // Storage operations
    pub storage_uploads_total: IntCounter,
    pub storage_deletes_total: IntCounter,

    // Cleanup operations
    pub cleanup_operations_total: IntCounter,
    pub backups_deleted_total: IntCounter,
}

impl Metrics {
    pub fn new() -> Result<Self> {
        let registry = Arc::new(Registry::new());

        // Backup operation counters
        let backups_total = IntCounter::new(
            "redis_vault_backups_total",
            "Total number of backup operations attempted",
        )?;

        let backups_successful = IntCounter::new(
            "redis_vault_backups_successful_total",
            "Total number of successful backup operations",
        )?;

        let backups_failed = IntCounter::new(
            "redis_vault_backups_failed_total",
            "Total number of failed backup operations",
        )?;

        // Backup operation details
        let backup_size_bytes = Histogram::with_opts(HistogramOpts::new(
            "redis_vault_backup_size_bytes",
            "Size of backup files in bytes",
        ))?;

        let backup_duration_seconds = Histogram::with_opts(HistogramOpts::new(
            "redis_vault_backup_duration_seconds",
            "Duration of backup operations in seconds",
        ))?;

        let last_backup_timestamp = Gauge::new(
            "redis_vault_last_backup_timestamp_seconds",
            "Unix timestamp of the last successful backup",
        )?;

        // Storage operations
        let storage_uploads_total = IntCounter::new(
            "redis_vault_storage_uploads_total",
            "Total number of storage upload operations by storage type",
        )?;

        let storage_deletes_total = IntCounter::new(
            "redis_vault_storage_deletes_total",
            "Total number of storage delete operations by storage type",
        )?;

        // Cleanup operations
        let cleanup_operations_total = IntCounter::new(
            "redis_vault_cleanup_operations_total",
            "Total number of cleanup operations performed",
        )?;

        let backups_deleted_total = IntCounter::new(
            "redis_vault_backups_deleted_total",
            "Total number of old backups deleted during cleanup",
        )?;

        // Register all metrics
        registry.register(Box::new(backups_total.clone()))?;
        registry.register(Box::new(backups_successful.clone()))?;
        registry.register(Box::new(backups_failed.clone()))?;
        registry.register(Box::new(backup_size_bytes.clone()))?;
        registry.register(Box::new(backup_duration_seconds.clone()))?;
        registry.register(Box::new(last_backup_timestamp.clone()))?;
        registry.register(Box::new(storage_uploads_total.clone()))?;
        registry.register(Box::new(storage_deletes_total.clone()))?;
        registry.register(Box::new(cleanup_operations_total.clone()))?;
        registry.register(Box::new(backups_deleted_total.clone()))?;

        Ok(Metrics {
            registry,
            backups_total,
            backups_successful,
            backups_failed,
            backup_size_bytes,
            backup_duration_seconds,
            last_backup_timestamp,
            storage_uploads_total,
            storage_deletes_total,
            cleanup_operations_total,
            backups_deleted_total,
        })
    }

    pub fn gather(&self) -> Result<String> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8(buffer)?)
    }
}

pub async fn start_metrics_server(
    metrics: Arc<RwLock<Metrics>>,
    port: u16,
    listen_address: String,
) -> Result<()> {
    let addr = listen_address
        .parse::<std::net::IpAddr>()
        .map_err(|e| anyhow::anyhow!("Invalid listen address: {}", e))?;

    let metrics_route = warp::path!("metrics").and(warp::get()).and_then(move || {
        let metrics = metrics.clone();
        async move {
            let metrics = metrics.read().await;
            match metrics.gather() {
                Ok(body) => Ok(warp::reply::with_header(
                    body,
                    "content-type",
                    "text/plain; charset=utf-8",
                )),
                Err(_) => Err(warp::reject::custom(MetricsError)),
            }
        }
    });

    let health_route = warp::path!("health")
        .and(warp::get())
        .map(|| warp::reply::with_status(String::from("OK"), warp::http::StatusCode::OK));

    let routes = metrics_route.or(health_route);

    tracing::info!("Starting metrics server on {}:{}", addr, port);
    warp::serve(routes).run((addr, port)).await;

    Ok(())
}

#[derive(Debug)]
struct MetricsError;

impl warp::reject::Reject for MetricsError {}
