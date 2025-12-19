use anyhow::Result;
use async_trait::async_trait;
use aws_sdk_s3::Client as S3Client;
use bytes::Bytes;
use chrono::{DateTime, Utc};

use super::{BackupMetadata, StorageBackend};
use crate::backup::BackupError;

pub struct S3Storage {
    client: S3Client,
}

impl S3Storage {
    pub async fn new() -> Result<Self> {
        let aws_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        let s3_config = aws_sdk_s3::config::Builder::from(&aws_config);
        let client = S3Client::from_conf(s3_config.build());

        Ok(S3Storage { client })
    }
}

#[async_trait]
impl StorageBackend for S3Storage {
    async fn upload(&self, bucket: &str, key: &str, data: Bytes) -> Result<()> {
        self.client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(data.into())
            .send()
            .await
            .map_err(|e| BackupError::S3(e.to_string()))?;

        Ok(())
    }

    async fn list(&self, bucket: &str, prefix: &str) -> Result<Vec<BackupMetadata>> {
        let mut backups = Vec::new();
        let mut continuation_token = None;

        loop {
            let mut request = self.client.list_objects_v2().bucket(bucket).prefix(prefix);

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

    async fn delete(&self, bucket: &str, key: &str) -> Result<()> {
        self.client
            .delete_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| BackupError::S3(e.to_string()))?;

        Ok(())
    }
}
