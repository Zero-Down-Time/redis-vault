use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::time;
use tracing::{debug, error, info, warn};

mod storage;
use storage::StorageBackend;
use storage::gcs::{GcsConfig, GcsStorage};
use storage::s3::{S3Config, S3Storage};

// Custom error types
#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("S3 error: {0}")]
    S3(String),
    #[error("GCS error: {0}")]
    Gcs(String),
    #[error("Configuration error: {0}")]
    Config(String),
}

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

// Configuration structures
#[derive(Debug, Deserialize, Serialize, Clone)]
struct Config {
    redis: RedisConfig,
    backup: BackupConfig,
    storage: StorageConfig,
    retention: RetentionConfig,
    logging: LoggingConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct RedisConfig {
    /// Redis connection string
    connection_string: String,
    /// Path to Redis data directory containing dump.rdb
    data_path: PathBuf,
    /// Name of this Redis node
    node_name: String,
    /// Backup from master nodes
    backup_master: bool,
    /// Backup from replica nodes
    backup_replica: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct BackupConfig {
    /// Interval between backup checks (e.g., "1h", "30m")
    interval: String,
    /// Filename pattern for dump file
    dump_filename: String,
    /// Initial delay to give Redis replication a chance to set up
    initial_delay: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type")]
enum StorageConfig {
    S3(S3Config),
    GCS(GcsConfig),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct RetentionConfig {
    /// Keep last N backups
    keep_last: usize,
    /// Keep backups newer than this duration (e.g., "7d", "30d")
    keep_duration: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct LoggingConfig {
    /// Log format: "text" or "json"
    format: String,
}

// Redis role detection
#[derive(Debug, PartialEq)]
enum RedisRole {
    Master,
    Replica,
    Unknown,
}

async fn get_redis_role(conn: &mut ConnectionManager) -> Result<RedisRole> {
    let info: String = redis::cmd("INFO")
        .arg("replication")
        .query_async(conn)
        .await?;

    for line in info.lines() {
        if line.starts_with("role:") {
            let role = line.split(':').nth(1).unwrap_or("").trim();
            return Ok(match role {
                "master" => RedisRole::Master,
                "slave" | "replica" => RedisRole::Replica,
                _ => RedisRole::Unknown,
            });
        }
    }

    Ok(RedisRole::Unknown)
}

// Backup manager
struct BackupManager {
    config: Config,
    storage: Arc<dyn StorageBackend>,
    redis_conn: Option<ConnectionManager>,
}

impl BackupManager {
    async fn new(config: Config) -> Result<Self> {
        // Create storage backend
        let storage: Arc<dyn StorageBackend> = match &config.storage {
            StorageConfig::S3(s3_config) => Arc::new(S3Storage::new(s3_config).await?),
            StorageConfig::GCS(gcs_config) => Arc::new(GcsStorage::new(gcs_config).await?),
        };

        // Create Redis connection if needed
        let redis_conn = if config.redis.backup_master != config.redis.backup_replica {
            let client = redis::Client::open(config.redis.connection_string.as_str())?;
            Some(ConnectionManager::new(client).await?)
        } else {
            None
        };

        Ok(BackupManager {
            config,
            storage,
            redis_conn,
        })
    }

    async fn should_backup(&mut self) -> Result<bool> {
        // If both master and replica backups are enabled, always backup
        if self.config.redis.backup_master && self.config.redis.backup_replica {
            return Ok(true);
        }

        // Get Redis role
        if let Some(conn) = &mut self.redis_conn {
            let role = get_redis_role(conn).await?;

            match role {
                RedisRole::Master => Ok(self.config.redis.backup_master),
                RedisRole::Replica => Ok(self.config.redis.backup_replica),
                RedisRole::Unknown => {
                    warn!("Could not determine Redis role, defaulting to backup");
                    Ok(true)
                }
            }
        } else {
            Ok(true)
        }
    }

    async fn perform_backup(&mut self) -> Result<()> {
        // Check if we should backup based on role
        if !self.should_backup().await? {
            info!("Skipping backup based on Redis role configuration");
            return Ok(());
        }

        // Construct dump file path
        let dump_path = self
            .config
            .redis
            .data_path
            .join(&self.config.backup.dump_filename);

        // Check if dump file exists
        if !dump_path.exists() {
            warn!("Dump file does not exist: {:?}", dump_path);
            return Ok(());
        }

        // Get file metadata
        let metadata = fs::metadata(&dump_path).await?;
        let modified = metadata.modified()?;

        // Read dump file
        info!("Reading dump file: {:?}", dump_path);
        let data = fs::read(&dump_path).await?;
        let data_bytes = bytes::Bytes::from(data);

        // Generate backup key
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let prefix = match &self.config.storage {
            StorageConfig::S3(cfg) => &cfg.prefix,
            StorageConfig::GCS(cfg) => &cfg.prefix,
        };

        let key = format!(
            "{}/{}_{}_{}.rdb",
            prefix.trim_end_matches('/'),
            self.config.redis.node_name,
            timestamp,
            modified.duration_since(std::time::UNIX_EPOCH)?.as_secs()
        );

        // Upload to storage
        info!("Uploading backup to: {}", key);
        self.storage.upload(&key, data_bytes).await?;
        info!("Backup uploaded successfully: {}", key);

        // Cleanup old backups
        self.cleanup_old_backups().await?;

        Ok(())
    }

    async fn cleanup_old_backups(&self) -> Result<()> {
        let prefix = match &self.config.storage {
            StorageConfig::S3(cfg) => &cfg.prefix,
            StorageConfig::GCS(cfg) => &cfg.prefix,
        };

        // List all backups for this node
        let node_prefix = format!(
            "{}/{}",
            prefix.trim_end_matches('/'),
            self.config.redis.node_name
        );

        let mut backups = self.storage.list(&node_prefix).await?;

        // Sort by timestamp (newest first)
        backups.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Determine which backups to keep
        let mut keep_indices = std::collections::HashSet::new();

        // Keep last N backups
        for i in 0..self.config.retention.keep_last.min(backups.len()) {
            keep_indices.insert(i);
        }

        // Keep backups within duration
        if let Some(duration_str) = &self.config.retention.keep_duration {
            let duration = humantime::parse_duration(duration_str)
                .map_err(|e| BackupError::Config(format!("Invalid duration: {}", e)))?;
            let cutoff = Utc::now() - chrono::Duration::from_std(duration)?;

            for (i, backup) in backups.iter().enumerate() {
                if backup.timestamp > cutoff {
                    keep_indices.insert(i);
                }
            }
        }

        // Delete backups not in keep set
        for (i, backup) in backups.iter().enumerate() {
            if !keep_indices.contains(&i) {
                info!("Deleting old backup: {}", backup.key);
                if let Err(e) = self.storage.delete(&backup.key).await {
                    error!("Failed to delete backup {}: {}", backup.key, e);
                }
            }
        }

        Ok(())
    }

    async fn run(&mut self, once: bool) -> Result<()> {
        let interval = humantime::parse_duration(&self.config.backup.interval)
            .map_err(|e| BackupError::Config(format!("Invalid interval: {}", e)))?;

        let initial_delay = humantime::parse_duration(&self.config.backup.initial_delay)
            .map_err(|e| BackupError::Config(format!("Invalid initial_delay: {}", e)))?;

        if initial_delay.as_secs() > 0 {
            info!(
                "Waiting for {} to allow Redis to setup replication",
                self.config.backup.initial_delay
            );
            time::sleep(initial_delay).await;
        }

        loop {
            match self.perform_backup().await {
                Ok(()) => debug!("Backup cycle completed successfully"),
                Err(e) => error!("Backup failed: {}", e),
            }

            if once {
                break;
            }

            time::sleep(interval).await;
        }

        Ok(())
    }
}

// Load configuration
async fn load_config(path: &Path) -> Result<Config> {
    // Start with default configuration
    let mut config = get_default_config();

    // Load from file if it exists
    if path.exists() {
        info!("Loading configuration from file: {:?}", path);
        let content = fs::read_to_string(path).await?;
        let file_config: Config = serde_json::from_str(&content)
            .or_else(|_| serde_yaml::from_str(&content))
            .context("Failed to parse configuration file")?;
        config = file_config;
    } else {
        info!("No config file found at {:?}, using defaults", path);
    }

    // Override with environment variables
    config = apply_env_overrides(config)?;

    Ok(config)
}

// Helper function to get default configuration
fn get_default_config() -> Config {
    Config {
        redis: RedisConfig {
            connection_string: "redis://localhost:6379".to_string(),
            data_path: PathBuf::from("/data"),
            node_name: "redis-node".to_string(),
            backup_master: true,
            backup_replica: true,
        },
        backup: BackupConfig {
            interval: "1h".to_string(),
            dump_filename: "dump.rdb".to_string(),
            initial_delay: "60s".to_string(),
        },
        storage: StorageConfig::S3(S3Config {
            bucket: "redis-vault".to_string(),
            prefix: "redis-vault".to_string(),
            region: None,
            endpoint: None,
        }),
        retention: RetentionConfig {
            keep_last: 7,
            keep_duration: None,
        },
        logging: LoggingConfig {
            format: "text".to_string(),
        },
    }
}

// Helper function to apply environment variable overrides
fn apply_env_overrides(mut config: Config) -> Result<Config> {
    // Redis configuration overrides
    if let Ok(conn_str) = std::env::var("REDIS_CONNECTION") {
        config.redis.connection_string = conn_str;
    }
    if let Ok(data_path) = std::env::var("REDIS_DATA_PATH") {
        config.redis.data_path = PathBuf::from(data_path);
    }
    if let Ok(node_name) = std::env::var("REDIS_NODE_NAME") {
        config.redis.node_name = node_name;
    }
    if let Ok(backup_master) = std::env::var("BACKUP_MASTER") {
        config.redis.backup_master = backup_master.parse().unwrap_or(true);
    }
    if let Ok(backup_replica) = std::env::var("BACKUP_REPLICA") {
        config.redis.backup_replica = backup_replica.parse().unwrap_or(true);
    }

    // Backup configuration overrides
    if let Ok(interval) = std::env::var("BACKUP_INTERVAL") {
        config.backup.interval = interval;
    }
    if let Ok(dump_filename) = std::env::var("DUMP_FILENAME") {
        config.backup.dump_filename = dump_filename;
    }
    if let Ok(initial_delay) = std::env::var("INITIAL_DELAY") {
        config.backup.initial_delay = initial_delay;
    }

    // Storage configuration overrides
    let storage_type = std::env::var("STORAGE_TYPE").unwrap_or_default();
    if storage_type == "gcs" || std::env::var("GCS_BUCKET").is_ok() {
        let bucket = std::env::var("GCS_BUCKET")
            .or_else(|_| {
                if let StorageConfig::GCS(ref gcs_config) = config.storage {
                    Ok(gcs_config.bucket.clone())
                } else {
                    Err(std::env::VarError::NotPresent)
                }
            })
            .context("GCS_BUCKET required for GCS storage")?;

        let prefix = std::env::var("GCS_PREFIX").unwrap_or_else(|_| {
            if let StorageConfig::GCS(ref gcs_config) = config.storage {
                gcs_config.prefix.clone()
            } else {
                "redis-vault".to_string()
            }
        });

        let project_id = std::env::var("GCS_PROJECT_ID").ok().or_else(|| {
            if let StorageConfig::GCS(ref gcs_config) = config.storage {
                gcs_config.project_id.clone()
            } else {
                None
            }
        });

        config.storage = StorageConfig::GCS(GcsConfig {
            bucket,
            prefix,
            project_id,
        });
    } else if std::env::var("S3_BUCKET").is_ok() || matches!(config.storage, StorageConfig::S3(_)) {
        let bucket = std::env::var("S3_BUCKET")
            .or_else(|_| {
                if let StorageConfig::S3(ref s3_config) = config.storage {
                    Ok(s3_config.bucket.clone())
                } else {
                    Err(std::env::VarError::NotPresent)
                }
            })
            .context("S3_BUCKET required for S3 storage")?;

        let prefix = std::env::var("S3_PREFIX").unwrap_or_else(|_| {
            if let StorageConfig::S3(ref s3_config) = config.storage {
                s3_config.prefix.clone()
            } else {
                "redis-vault".to_string()
            }
        });

        let region = std::env::var("AWS_REGION").ok().or_else(|| {
            if let StorageConfig::S3(ref s3_config) = config.storage {
                s3_config.region.clone()
            } else {
                None
            }
        });

        let endpoint = std::env::var("S3_ENDPOINT").ok().or_else(|| {
            if let StorageConfig::S3(ref s3_config) = config.storage {
                s3_config.endpoint.clone()
            } else {
                None
            }
        });

        config.storage = StorageConfig::S3(S3Config {
            bucket,
            prefix,
            region,
            endpoint,
        });
    }

    // Retention configuration overrides
    if let Ok(keep_last) = std::env::var("RETENTION_KEEP_LAST") {
        config.retention.keep_last = keep_last.parse().unwrap_or(7);
    }
    if let Ok(keep_duration) = std::env::var("RETENTION_KEEP_DURATION") {
        config.retention.keep_duration = Some(keep_duration);
    }

    // Logging configuration overrides
    if let Ok(log_format) = std::env::var("LOG_FORMAT") {
        config.logging.format = log_format;
    }

    Ok(config)
}

fn init_logging(config: &Config) {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    match config.logging.format.as_str() {
        "json" => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .json()
                .flatten_event(true)
                .without_time()
                .init();
        }
        _ => {
            tracing_subscriber::fmt().with_env_filter(env_filter).init();
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse arguments
    let args = Args::parse();

    // Load configuration
    let config = load_config(&args.config).await?;

    // Initialize tracing with config
    init_logging(&config);

    info!("Configuration loaded successfully");
    debug!("Config: {:?}", config);

    // Create and run backup manager
    let mut manager = BackupManager::new(config).await?;
    info!("Backup manager initialized");

    manager.run(args.once).await?;

    Ok(())
}
