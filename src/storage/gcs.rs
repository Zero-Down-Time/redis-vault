use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use gcloud_storage::client::{Client as GcsClient, ClientConfig};

use super::{BackupMetadata, StorageBackend};
use crate::backup::BackupError;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct GcsConfig {
    pub bucket: String,
    pub prefix: String,
    pub project_id: Option<String>,
}

pub struct GcsStorage {
    client: GcsClient,
    bucket: String,
}

impl GcsStorage {
    pub async fn new(config: &GcsConfig) -> Result<Self> {
        let client_config = ClientConfig::default()
            .with_auth()
            .await
            .map_err(|e| BackupError::Gcs(e.to_string()))?;

        let client = GcsClient::new(client_config);

        Ok(GcsStorage {
            client,
            bucket: config.bucket.clone(),
        })
    }
}

#[async_trait]
impl StorageBackend for GcsStorage {
    async fn upload(&self, key: &str, data: Bytes) -> Result<()> {
        use gcloud_storage::http::objects::upload::{Media, UploadObjectRequest, UploadType};

        let upload_type = UploadType::Simple(Media::new(key.to_string()));
        let req = UploadObjectRequest {
            bucket: self.bucket.clone(),
            ..Default::default()
        };

        self.client
            .upload_object(&req, data.to_vec(), &upload_type)
            .await
            .map_err(|e| BackupError::Gcs(e.to_string()))?;

        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<BackupMetadata>> {
        use gcloud_storage::http::objects::list::ListObjectsRequest;

        let req = ListObjectsRequest {
            bucket: self.bucket.clone(),
            prefix: Some(prefix.to_string()),
            ..Default::default()
        };

        let objects = self
            .client
            .list_objects(&req)
            .await
            .map_err(|e| BackupError::Gcs(e.to_string()))?;

        let mut backups = Vec::new();

        if let Some(items) = objects.items {
            for object in items {
                if let Some(time_created) = object.time_created {
                    let timestamp =
                        DateTime::<Utc>::from_timestamp(time_created.unix_timestamp(), 0)
                            .unwrap_or_else(Utc::now);

                    backups.push(BackupMetadata {
                        key: object.name,
                        timestamp,
                        size: object.size,
                    });
                }
            }
        }

        Ok(backups)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        use gcloud_storage::http::objects::delete::DeleteObjectRequest;

        let req = DeleteObjectRequest {
            bucket: self.bucket.clone(),
            object: key.to_string(),
            ..Default::default()
        };

        self.client
            .delete_object(&req)
            .await
            .map_err(|e| BackupError::Gcs(e.to_string()))?;

        Ok(())
    }
}
