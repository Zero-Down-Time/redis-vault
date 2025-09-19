use anyhow::Result;
use async_trait::async_trait;
use aws_sdk_s3::Client as S3Client;
use bytes::Bytes;
use chrono::{DateTime, Utc};

use super::{BackupMetadata, StorageBackend};
use crate::BackupError;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct S3Config {
    pub bucket: String,
    pub prefix: String,
    pub region: Option<String>,
    pub endpoint: Option<String>,
}

pub struct S3Storage {
    client: S3Client,
    bucket: String,
}

impl S3Storage {
    pub async fn new(config: &S3Config) -> Result<Self> {
        let mut aws_config = aws_config::defaults(aws_config::BehaviorVersion::latest());

        if let Some(region) = &config.region {
            aws_config = aws_config.region(aws_config::Region::new(region.clone()));
        }

        let aws_config = aws_config.load().await;
        let mut s3_config = aws_sdk_s3::config::Builder::from(&aws_config);

        if let Some(endpoint) = &config.endpoint {
            s3_config = s3_config.endpoint_url(endpoint);
        }

        let client = S3Client::from_conf(s3_config.build());

        Ok(S3Storage {
            client,
            bucket: config.bucket.clone(),
        })
    }
}

#[async_trait]
impl StorageBackend for S3Storage {
    async fn upload(&self, key: &str, data: Bytes) -> Result<()> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(data.into())
            .send()
            .await
            .map_err(|e| BackupError::S3(e.to_string()))?;

        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<BackupMetadata>> {
        let mut backups = Vec::new();
        let mut continuation_token = None;

        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix);

            if let Some(token) = continuation_token {
                request = request.continuation_token(token);
            }

            let response = request
                .send()
                .await
                .map_err(|e| BackupError::S3(e.to_string()))?;

            if let Some(contents) = response.contents {
                for object in contents {
                    if let (Some(key), Some(last_modified)) = (object.key, object.last_modified) {
                        backups.push(BackupMetadata {
                            key,
                            timestamp: DateTime::from_timestamp(last_modified.secs(), 0)
                                .unwrap_or_else(Utc::now),
                            size: object.size.unwrap_or(0),
                        });
                    }
                }
            }

            if response.is_truncated.unwrap_or(false) {
                continuation_token = response.next_continuation_token;
            } else {
                break;
            }
        }

        Ok(backups)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| BackupError::S3(e.to_string()))?;

        Ok(())
    }
}
