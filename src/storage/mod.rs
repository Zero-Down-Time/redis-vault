use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};

pub mod gcs;
pub mod s3;

#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn upload(&self, key: &str, data: Bytes) -> Result<()>;
    async fn list(&self, prefix: &str) -> Result<Vec<BackupMetadata>>;
    async fn delete(&self, key: &str) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct BackupMetadata {
    pub key: String,
    pub timestamp: DateTime<Utc>,
    #[allow(dead_code)]
    pub size: i64,
}
