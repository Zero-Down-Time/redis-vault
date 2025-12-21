use anyhow::Result;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use prometheus::{Encoder, Gauge, Histogram, HistogramOpts, IntCounter, Registry, TextEncoder};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;

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

/// Start the metrics HTTP server using Hyper 1.x
pub async fn start_metrics_server(
    metrics: Arc<RwLock<Metrics>>,
    port: u16,
    listen_address: String,
) -> Result<()> {
    let addr = listen_address
        .parse::<std::net::IpAddr>()
        .map_err(|e| anyhow::anyhow!("Invalid listen address: {}", e))?;

    let sock_addr = SocketAddr::new(addr, port);

    // Create TCP listener first - this will fail immediately if port is in use
    let listener = TcpListener::bind(&sock_addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind metrics server to {}: {}", sock_addr, e))?;

    let local_addr = listener.local_addr()?;
    tracing::info!("Metrics server bound to {}", local_addr);

    // Accept connections in a loop
    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let metrics = metrics.clone();

        // Spawn a task to handle each connection
        tokio::spawn(async move {
            // Create a service function that handles requests for this connection
            let service = service_fn(move |req| {
                let metrics = metrics.clone();
                async move { handle_request(req, metrics).await }
            });

            // Serve HTTP/1.1 requests on this connection
            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                tracing::error!("Error serving connection: {:?}", err);
            }
        });
    }
}

/// Handle incoming HTTP requests for metrics and health endpoints
async fn handle_request(
    req: Request<hyper::body::Incoming>,
    metrics: Arc<RwLock<Metrics>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    match (req.method(), req.uri().path()) {
        // GET /metrics - Prometheus metrics endpoint
        (&Method::GET, "/metrics") => {
            let metrics = metrics.read().await;
            match metrics.gather() {
                Ok(body) => Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(Full::new(Bytes::from(body)))
                    .unwrap()),
                Err(e) => {
                    tracing::error!("Failed to gather metrics: {}", e);
                    Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Full::new(Bytes::from("Failed to gather metrics")))
                        .unwrap())
                }
            }
        }

        // GET /health - Health check endpoint
        (&Method::GET, "/health") => Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Full::new(Bytes::from("OK")))
            .unwrap()),

        // 404 Not Found for all other routes
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::from("Not Found")))
            .unwrap()),
    }
}
