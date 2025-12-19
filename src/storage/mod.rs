use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use std::fmt;
use std::sync::Arc;

use crate::storage::{gcs::GcsStorage, s3::S3Storage};

pub mod gcs;
pub mod s3;

#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn upload(&self, bucket: &str, key: &str, data: Bytes) -> Result<()>;
    async fn list(&self, bucket: &str, prefix: &str) -> Result<Vec<BackupMetadata>>;
    async fn delete(&self, bucket: &str, key: &str) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct BackupMetadata {
    pub key: String,
    pub timestamp: DateTime<Utc>,
    #[allow(dead_code)]
    pub size: i64,
}

#[derive(Debug, Clone)]
pub enum StorageType {
    S3,
    GS,
}

/// Storage URL, "(s3|gs)://bucket</prefix>"
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub storage_type: StorageType,
    pub bucket: String,
    pub prefix: String,
}

#[derive(Debug)]
pub struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Parse error: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

pub async fn get_storage_client(storage_type: &StorageType) -> Result<Arc<dyn StorageBackend>> {
    let client: Arc<dyn StorageBackend> = match storage_type {
        StorageType::S3 => Arc::new(S3Storage::new().await?),
        StorageType::GS => Arc::new(GcsStorage::new().await?),
    };

    Ok(client)
}

pub fn parse_storage_url(url: &str) -> Result<StorageConfig, ParseError> {
    // Split on "://"
    let parts: Vec<&str> = url.split("://").collect();
    if parts.len() != 2 {
        return Err(ParseError("Invalid format: missing '://'".to_string()));
    }

    // Parse storage type
    let storage_type = match parts[0] {
        "s3" => StorageType::S3,
        "gs" => StorageType::GS,
        _ => return Err(ParseError(format!("Invalid storage type: {}", parts[0]))),
    };

    // Split bucket and prefix
    let path_parts: Vec<&str> = parts[1].splitn(2, '/').collect();

    let bucket = path_parts[0].to_string();
    if bucket.is_empty() {
        return Err(ParseError("Invalid format: empty bucket name".to_string()));
    }

    // If no prefix provided, default to "/"
    let prefix = if path_parts.len() == 1 {
        "/".to_string()
    } else {
        path_parts[1].to_string()
    };

    Ok(StorageConfig {
        storage_type,
        bucket,
        prefix,
    })
}
