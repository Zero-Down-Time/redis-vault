use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::storage::gcs::GcsConfig;
use crate::storage::s3::S3Config;

const DEFAULT_BACKUP_MASTER: bool = true;
const DEFAULT_BACKUP_REPLICA: bool = true;
const DEFAULT_RETENTION_COUNT: usize = 7;
const DEFAULT_METRICS_PORT: u16 = 9090;
const DEFAULT_INTERVAL: &str = "1h";
const DEFAULT_INITIAL_DELAY: &str = "300s";

// Configuration structures
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub redis: RedisConfig,
    pub backup: BackupConfig,
    pub storage: StorageConfig,
    pub retention: RetentionConfig,
    pub logging: LoggingConfig,
    pub metrics: MetricsConfig,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct RedisConfig {
    /// Redis connection string
    pub connection_string: String,
    /// Path to Redis data directory containing dump.rdb
    pub data_path: PathBuf,
    /// Name of this Redis node
    pub node_name: String,
    /// Backup from master nodes
    pub backup_master: bool,
    /// Backup from replica nodes
    pub backup_replica: bool,
}

// Custom Debug for potentially sensitive connection_string
impl fmt::Debug for RedisConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisConfig")
            .field("connection_string", &"[REDACTED]")
            .field("data_path", &self.data_path)
            .field("node_name", &self.node_name)
            .field("backup_master", &self.backup_master)
            .field("backup_replica", &self.backup_replica)
            .finish()
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BackupConfig {
    /// Interval between backup checks (e.g., "1h", "30m")
    pub interval: String,
    /// Filename pattern for dump file
    pub dump_filename: String,
    /// Initial delay to give Redis replication a chance to set up
    pub initial_delay: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type")]
pub enum StorageConfig {
    S3(S3Config),
    Gcs(GcsConfig),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RetentionConfig {
    /// Keep last N backups
    pub keep_last: usize,
    /// Keep backups newer than this duration (e.g., "7d", "30d")
    pub keep_duration: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LoggingConfig {
    /// Log format: "text" or "json"
    pub format: String,
    // debug, error, info, warn
    pub level: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MetricsConfig {
    /// Enable metrics endpoint
    pub enabled: bool,
    /// Port for metrics server
    pub port: u16,
    /// Listen address for metrics server
    pub listen_address: String,
}

/// Load configuration from file with environment variable overrides
pub fn load_config(path: &Path) -> Result<Config> {
    // Start with default configuration
    let mut config = get_default_config();

    // Load from file if it exists
    if path.exists() {
        info!("Loading configuration from file: {:?}", path);
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read configuration file: {:?}", path))?;
        let file_config: Config = serde_json::from_str(&content)
            .or_else(|_| serde_yaml::from_str(&content))
            .context("Failed to parse configuration file")?;
        config = file_config;
    } else {
        warn!("No config file found at {:?}, using defaults", path);
    }

    // Override with environment variables
    config = apply_env_overrides(config)?;

    Ok(config)
}

/// Get default configuration values
pub fn get_default_config() -> Config {
    Config {
        redis: RedisConfig {
            connection_string: "redis://localhost:6379".to_string(),
            data_path: PathBuf::from("/data"),
            node_name: "redis-node".to_string(),
            backup_master: DEFAULT_BACKUP_MASTER,
            backup_replica: DEFAULT_BACKUP_REPLICA,
        },
        backup: BackupConfig {
            interval: DEFAULT_INTERVAL.to_string(),
            dump_filename: "dump.rdb".to_string(),
            initial_delay: DEFAULT_INITIAL_DELAY.to_string(),
        },
        storage: StorageConfig::S3(S3Config {
            bucket: "redis-vault".to_string(),
            prefix: "redis-vault".to_string(),
            region: None,
            endpoint: None,
        }),
        retention: RetentionConfig {
            keep_last: DEFAULT_RETENTION_COUNT,
            keep_duration: None,
        },
        logging: LoggingConfig {
            format: "text".to_string(),
            level: "info".to_string(),
        },
        metrics: MetricsConfig {
            enabled: false,
            port: DEFAULT_METRICS_PORT,
            listen_address: "0.0.0.0".to_string(),
        },
    }
}

/// Apply environment variable overrides to configuration
pub fn apply_env_overrides(mut config: Config) -> Result<Config> {
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
                if let StorageConfig::Gcs(ref gcs_config) = config.storage {
                    Ok(gcs_config.bucket.clone())
                } else {
                    Err(std::env::VarError::NotPresent)
                }
            })
            .context("GCS_BUCKET required for GCS storage")?;

        let prefix = std::env::var("GCS_PREFIX").unwrap_or_else(|_| {
            if let StorageConfig::Gcs(ref gcs_config) = config.storage {
                gcs_config.prefix.clone()
            } else {
                "redis-vault".to_string()
            }
        });

        let project_id = std::env::var("GCS_PROJECT_ID").ok().or_else(|| {
            if let StorageConfig::Gcs(ref gcs_config) = config.storage {
                gcs_config.project_id.clone()
            } else {
                None
            }
        });

        config.storage = StorageConfig::Gcs(GcsConfig {
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
    if let Ok(log_level) = std::env::var("LOG_LEVEL") {
        config.logging.level = log_level;
    }

    // Metrics configuration overrides
    if let Ok(metrics_enabled) = std::env::var("METRICS_ENABLED") {
        config.metrics.enabled = metrics_enabled.parse().unwrap_or(true);
    }
    if let Ok(metrics_port) = std::env::var("METRICS_PORT") {
        config.metrics.port = metrics_port.parse().unwrap_or(9090);
    }
    if let Ok(metrics_address) = std::env::var("METRICS_LISTEN_ADDRESS") {
        config.metrics.listen_address = metrics_address;
    }

    Ok(config)
}
