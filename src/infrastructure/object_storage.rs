//! Bounded S3 operations and signing against distinct, unmodified public endpoints.

use std::{collections::BTreeMap, future::Future, time::Duration};

use aws_sdk_s3::{
    Client,
    config::{
        BehaviorVersion, Credentials, Region, RequestChecksumCalculation,
        ResponseChecksumValidation,
    },
    presigning::PresigningConfig,
    primitives::ByteStream,
};

use super::config::MediaStorageConfig;
use crate::error::AppError;

pub const STORAGE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct ObjectStorage {
    internal: Client,
    public: Client,
    bucket: String,
}

pub struct ObjectHead {
    pub size_bytes: u64,
    pub etag: String,
}

pub struct SignedRequest {
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

async fn bounded<T>(operation: impl Future<Output = Result<T, AppError>>) -> Result<T, AppError> {
    tokio::time::timeout(STORAGE_TIMEOUT, operation)
        .await
        .map_err(|_| AppError::unavailable())?
}

impl ObjectStorage {
    /// Remove only an isolated browser-test bucket after its application writers stop.
    /// A production bucket name cannot satisfy this guard, even through the operator CLI.
    pub async fn destroy_browser_test_bucket(&self) -> Result<(), AppError> {
        let suffix = self
            .bucket
            .strip_prefix("chat-e2e-")
            .ok_or_else(AppError::forbidden)?;
        if suffix.len() != 32 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(AppError::forbidden());
        }

        match bounded(async {
            match self
                .internal
                .head_bucket()
                .bucket(&self.bucket)
                .send()
                .await
            {
                Ok(_) => Ok(true),
                Err(error)
                    if error
                        .raw_response()
                        .is_some_and(|response| response.status().as_u16() == 404) =>
                {
                    Ok(false)
                }
                Err(_) => Err(AppError::unavailable()),
            }
        })
        .await?
        {
            true => {}
            false => return Ok(()),
        }

        loop {
            let page = bounded(async {
                self.internal
                    .list_objects_v2()
                    .bucket(&self.bucket)
                    .send()
                    .await
                    .map_err(|_| AppError::unavailable())
            })
            .await?;
            if page.contents().is_empty() {
                break;
            }
            for object in page.contents() {
                if let Some(key) = object.key() {
                    self.delete(key).await?;
                }
            }
        }
        bounded(async {
            self.internal
                .delete_bucket()
                .bucket(&self.bucket)
                .send()
                .await
                .map_err(|_| AppError::unavailable())?;
            Ok(())
        })
        .await
    }

    /// Create the configured private bucket only when it is absent; dependency and
    /// permission failures remain errors rather than being mistaken for missing data.
    pub async fn ensure_bucket(&self) -> Result<(), AppError> {
        bounded(async {
            match self
                .internal
                .head_bucket()
                .bucket(&self.bucket)
                .send()
                .await
            {
                Ok(_) => Ok(()),
                Err(error)
                    if error
                        .raw_response()
                        .is_some_and(|response| response.status().as_u16() == 404) =>
                {
                    self.internal
                        .create_bucket()
                        .bucket(&self.bucket)
                        .send()
                        .await
                        .map_err(|_| AppError::unavailable())?;
                    Ok(())
                }
                Err(_) => Err(AppError::unavailable()),
            }
        })
        .await
    }

    /// Build clients without discovery or a network call, so outages never block text chat.
    pub fn new(config: &MediaStorageConfig) -> Self {
        let client = |endpoint: &str| {
            let sdk = aws_sdk_s3::Config::builder()
                .behavior_version(BehaviorVersion::latest())
                .region(Region::new(config.region.clone()))
                .credentials_provider(Credentials::new(
                    &config.access_key,
                    &config.secret_key,
                    None,
                    None,
                    "explicit-media",
                ))
                .endpoint_url(endpoint)
                .force_path_style(true)
                .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
                .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
                .build();
            Client::from_conf(sdk)
        };
        Self {
            internal: client(&config.internal_endpoint),
            public: client(&config.public_endpoint),
            bucket: config.bucket.clone(),
        }
    }

    /// Sign only a staging key; callers must never pass a permanent candidate key.
    pub async fn sign_upload(
        &self,
        key: &str,
        content_type: &str,
        size: u64,
        lifetime: Duration,
    ) -> Result<SignedRequest, AppError> {
        if !key.starts_with("staging/") {
            return Err(AppError::forbidden());
        }
        bounded(async {
            let config =
                PresigningConfig::expires_in(lifetime).map_err(|_| AppError::unavailable())?;
            let request = self
                .public
                .put_object()
                .bucket(&self.bucket)
                .key(key)
                .content_type(content_type)
                .content_length(size as i64)
                .presigned(config)
                .await
                .map_err(|_| AppError::unavailable())?;
            Ok(SignedRequest {
                url: request.uri().to_owned(),
                headers: browser_headers(request.headers()),
            })
        })
        .await
    }

    /// Sign an authorized immutable object with safe response metadata and no cookies.
    pub async fn sign_download(
        &self,
        key: &str,
        content_type: &str,
        disposition: &str,
        lifetime: Duration,
    ) -> Result<SignedRequest, AppError> {
        bounded(async {
            let config =
                PresigningConfig::expires_in(lifetime).map_err(|_| AppError::unavailable())?;
            let request = self
                .public
                .get_object()
                .bucket(&self.bucket)
                .key(key)
                .response_content_type(content_type)
                .response_content_disposition(disposition)
                .response_cache_control("private, no-store")
                .presigned(config)
                .await
                .map_err(|_| AppError::unavailable())?;
            Ok(SignedRequest {
                url: request.uri().to_owned(),
                headers: browser_headers(request.headers()),
            })
        })
        .await
    }

    /// Seal size and ETag; no body or caller MIME metadata is trusted.
    pub async fn head(&self, key: &str) -> Result<ObjectHead, AppError> {
        bounded(async {
            let result = self
                .internal
                .head_object()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
                .map_err(|_| AppError::unavailable())?;
            Ok(ObjectHead {
                size_bytes: result
                    .content_length()
                    .and_then(|size| u64::try_from(size).ok())
                    .ok_or_else(AppError::unavailable)?,
                etag: result
                    .e_tag()
                    .filter(|etag| !etag.is_empty())
                    .ok_or_else(AppError::unavailable)?
                    .to_owned(),
            })
        })
        .await
    }

    /// Conditional copying isolates the exact sealed staging version from later PUTs.
    pub async fn copy(&self, source: &str, etag: &str, candidate: &str) -> Result<(), AppError> {
        bounded(async {
            self.internal
                .copy_object()
                .bucket(&self.bucket)
                .key(candidate)
                .copy_source(format!("{}/{}", self.bucket, source))
                .copy_source_if_match(etag)
                .metadata_directive(aws_sdk_s3::types::MetadataDirective::Replace)
                .content_type("application/octet-stream")
                .cache_control("private, no-store")
                .send()
                .await
                .map_err(|_| AppError::unavailable())?;
            Ok(())
        })
        .await
    }

    /// The timeout includes streaming consumption, with a separate cap before each append.
    pub async fn read(&self, key: &str, cap: u64) -> Result<Vec<u8>, AppError> {
        bounded(async {
            let response = self
                .internal
                .get_object()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
                .map_err(|_| AppError::unavailable())?;
            if response
                .content_length()
                .is_some_and(|length| length < 0 || length as u64 > cap)
            {
                return Err(AppError::invalid("媒体文件超过限制"));
            }
            let mut body = response.body;
            let mut bytes = Vec::new();
            while let Some(chunk) = body.try_next().await.map_err(|_| AppError::unavailable())? {
                if (bytes.len() as u64).saturating_add(chunk.len() as u64) > cap {
                    return Err(AppError::invalid("媒体文件超过限制"));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        })
        .await
    }

    /// Write sanitized derived bytes only to a newly recorded, lease-specific key.
    pub async fn write(
        &self,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<(), AppError> {
        bounded(async {
            self.internal
                .put_object()
                .bucket(&self.bucket)
                .key(key)
                .body(ByteStream::from(bytes))
                .content_type(content_type)
                .cache_control("private, no-store")
                .send()
                .await
                .map_err(|_| AppError::unavailable())?;
            Ok(())
        })
        .await
    }

    /// S3 deletion is idempotent for absent keys; bucket and durable transport errors propagate.
    pub async fn delete(&self, key: &str) -> Result<(), AppError> {
        bounded(async {
            self.internal
                .delete_object()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
                .map_err(|_| AppError::unavailable())?;
            Ok(())
        })
        .await
    }
}

fn browser_headers<'a>(
    headers: impl Iterator<Item = (&'a str, &'a str)>,
) -> BTreeMap<String, String> {
    headers
        .filter(|(name, _)| {
            !matches!(
                name.to_ascii_lowercase().as_str(),
                "host" | "content-length"
            )
        })
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}
