//! Backup manager module for Redis Vault
//!
//! This module handles the core backup logic including:
//! - Redis role detection
//! - Backup scheduling and execution
//! - Retention policy enforcement
//! - Storage backend interaction

use anyhow::Result;
use chrono::Utc;
use redis::aio::ConnectionManager;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::fs;
use tokio::sync::RwLock;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::metrics::Metrics;
use crate::storage::{StorageConfig, get_storage_client, parse_storage_url};

/// Custom error types for backup operations
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

/// Redis replication role
#[derive(Debug, PartialEq)]
enum RedisRole {
    Master,
    Replica,
    Unknown,
}

/// Detect the current Redis role (master or replica) by querying the INFO command
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

/// BackupManager handles the backup lifecycle including:
/// - Checking if backups should be performed based on Redis role
/// - Reading and uploading dump files to storage
/// - Cleaning up old backups based on retention policy
pub struct BackupManager {
    config: Config,
    storage: StorageConfig,
    metrics: Arc<RwLock<Metrics>>,
}

impl BackupManager {
    /// Create a new BackupManager instance
    ///
    /// This initializes the storage backend and optionally creates a Redis connection
    /// for role detection (only needed if backup_master != backup_replica).
    pub async fn new(config: Config, metrics: Arc<RwLock<Metrics>>) -> Result<Self> {
        let storage = parse_storage_url(&config.backup.storage_url)?;

        Ok(BackupManager {
            config,
            storage,
            metrics,
        })
    }

    /// Check if a backup should be performed based on Redis role configuration
    async fn should_backup(&mut self) -> Result<bool> {
        // If both master and replica backups are enabled, always backup
        if self.config.redis.backup_master && self.config.redis.backup_replica {
            return Ok(true);
        }

        // Get Redis role
        // Create Redis connection if needed for role detection
        if self.config.redis.backup_master || self.config.redis.backup_replica {
            let client = redis::Client::open(self.config.redis.connection_string.as_str())?;
            let conn = &mut ConnectionManager::new(client).await?;

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
            Ok(false)
        }
    }

    /// Perform a single backup operation
    ///
    /// This method:
    /// 1. Checks if backup should be performed based on role
    /// 2. Reads the dump file from disk
    /// 3. Uploads it to the configured storage backend
    /// 4. Cleans up old backups based on retention policy
    pub async fn perform_backup(&mut self) -> Result<()> {
        let start_time = Instant::now();
        let metrics = self.metrics.write().await;
        metrics.backups_total.inc();
        drop(metrics);

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

        let backup_result = async {
            // Get file metadata
            let metadata = fs::metadata(&dump_path).await?;
            let modified = metadata.modified()?;

            // Read dump file
            debug!("Reading dump file: {:?}", dump_path);
            let data = fs::read(&dump_path).await?;
            let data_size = data.len() as f64;
            let data_bytes = bytes::Bytes::from(data);

            let key = format!(
                "{}/{}_{}.rdb",
                self.storage.prefix.trim_end_matches('/'),
                self.config.redis.node_name,
                humantime::format_rfc3339_seconds(modified)
            );

            // Upload to storage
            debug!("Uploading backup to: {}", key);

            let client = get_storage_client(&self.storage.storage_type).await?;

            match client.upload(&self.storage.bucket, &key, data_bytes).await {
                Ok(()) => {
                    info!("Backup uploaded successfully: {}", key);

                    // Record successful upload metrics
                    let metrics = self.metrics.write().await;
                    metrics.storage_uploads_total.inc();
                    metrics.backup_size_bytes.observe(data_size);
                    metrics
                        .last_backup_timestamp
                        .set(Utc::now().timestamp() as f64);

                    Ok(())
                }
                Err(e) => {
                    let metrics = self.metrics.write().await;
                    metrics.storage_uploads_total.inc();
                    Err(e)
                }
            }
        }
        .await;

        // Record backup operation metrics
        let duration = start_time.elapsed().as_secs_f64();
        let metrics = self.metrics.write().await;
        metrics.backup_duration_seconds.observe(duration);

        match backup_result {
            Ok(()) => {
                metrics.backups_successful.inc();
                drop(metrics);
                Ok(())
            }
            Err(e) => {
                metrics.backups_failed.inc();
                Err(e)
            }
        }
    }

    /// Clean up old backups based on retention policy
    ///
    /// Keeps backups that satisfy either:
    /// - Are within the `keep_last` count
    /// - Are newer than `keep_duration`
    async fn cleanup_old_backups(&self) -> Result<()> {
        let metrics = self.metrics.write().await;
        metrics.cleanup_operations_total.inc();
        drop(metrics);

        // List all backups for this node
        let node_prefix = format!(
            "{}/{}",
            self.storage.prefix.trim_end_matches('/'),
            self.config.redis.node_name
        );

        let client = get_storage_client(&self.storage.storage_type).await?;

        let mut backups = client.list(&self.storage.bucket, &node_prefix).await?;

        // Sort by timestamp (newest first)
        backups.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Determine which backups to keep
        let mut keep_indices = HashSet::new();

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
        let mut deleted_count = 0;
        for (i, backup) in backups.iter().enumerate() {
            if !keep_indices.contains(&i) {
                info!("Deleting old backup: {}", backup.key);

                let metrics = self.metrics.write().await;
                match client.delete(&self.storage.bucket, &backup.key).await {
                    Ok(()) => {
                        metrics.storage_deletes_total.inc();
                        deleted_count += 1;
                    }
                    Err(e) => {
                        error!("Failed to delete backup {}: {}", backup.key, e);
                        metrics.storage_deletes_total.inc();
                    }
                }
                drop(metrics);
            }
        }

        if deleted_count > 0 {
            let metrics = self.metrics.write().await;
            for _ in 0..deleted_count {
                metrics.backups_deleted_total.inc();
            }
        }

        Ok(())
    }

    /// Run the backup manager loop
    ///
    /// This method:
    /// 1. Waits for initial_delay to allow Redis replication to stabilize
    /// 2. Schedules backups at fixed intervals aligned to Unix timestamps
    /// 3. Runs continuously unless `once` is true (for testing)
    pub async fn run(&mut self, once: bool) -> Result<()> {
        let interval = humantime::parse_duration(&self.config.backup.interval)
            .map_err(|e| BackupError::Config(format!("Invalid interval: {}", e)))?;

        let initial_delay = humantime::parse_duration(&self.config.backup.initial_delay)
            .map_err(|e| BackupError::Config(format!("Invalid initial_delay: {}", e)))?;

        if !initial_delay.is_zero() {
            info!(
                "Initially waiting for {} to allow for Redis to setup replication",
                self.config.backup.initial_delay
            );
            time::sleep(initial_delay).await;
        }

        loop {
            if !once {
                // calculate seconds till next execution time slot using UNIX timestamp as reference
                let next_interval = Duration::new(
                    (interval.as_secs() as i64 - Utc::now().timestamp() % interval.as_secs() as i64)
                        as u64,
                    0,
                );

                info!(
                    "Next backup at {}",
                    humantime::format_rfc3339_seconds(SystemTime::now() + next_interval)
                );

                // wait for remaining time
                time::sleep(next_interval).await;
            }

            match self.perform_backup().await {
                Ok(()) => {
                    debug!("Backup cycle completed successfully");
                }
                Err(e) => error!("Backup failed: {}", e),
            }

            // Cleanup old backups
            match self.cleanup_old_backups().await {
                Ok(()) => {
                    debug!("Backup retention run successfully");
                }
                Err(e) => error!("Backup retention failed: {}", e),
            }

            if once {
                break;
            }
        }

        Ok(())
    }
}
