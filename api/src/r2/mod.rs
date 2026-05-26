//! Cloudflare R2 client. R2 is S3-compatible, so we use `aws-sdk-s3`
//! with the endpoint overridden. Per SAAS-PLAN §12.3 we pick R2 over
//! S3 for zero egress fees — wiki reads dominate the cost profile.

use crate::config::R2Config;
use aws_sdk_s3::{
    config::{Credentials, Region},
    primitives::ByteStream,
    Client,
};
use std::time::Duration;

/// Build an R2 client. Region is irrelevant for R2 but the SDK requires
/// one; we use `auto`. Credentials are sourced from our config struct,
/// not from the AWS credential chain, so there's no accidental fallback
/// to `~/.aws/credentials` on a dev laptop.
pub fn build_client(r2: &R2Config) -> Client {
    let credentials = Credentials::new(
        &r2.access_key_id,
        &r2.secret_access_key,
        None,
        None,
        "coral-r2-config",
    );

    let config = aws_sdk_s3::Config::builder()
        .endpoint_url(&r2.endpoint)
        .region(Region::new("auto"))
        .credentials_provider(credentials)
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .force_path_style(true) // R2 expects path-style addressing.
        .build();

    Client::from_conf(config)
}

/// Fetch an object body as a `Vec<u8>`. Use for small artifacts only —
/// wiki pages are markdown (~few KB), so this is fine. For tarballs
/// we'd stream via `into_async_read()`.
pub async fn get_object(client: &Client, bucket: &str, key: &str) -> Result<Vec<u8>, R2Error> {
    let resp = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| match e.into_service_error() {
            err if err.is_no_such_key() => R2Error::NotFound,
            err => R2Error::Other(anyhow::anyhow!("{}", err)),
        })?;

    let bytes = resp
        .body
        .collect()
        .await
        .map_err(|e| R2Error::Other(anyhow::anyhow!("read body: {}", e)))?
        .into_bytes();
    Ok(bytes.to_vec())
}

/// Pre-sign a GET URL valid for `ttl`. Worker uses these to download
/// the wiki tarball without ever holding R2 credentials.
pub async fn presigned_get(
    client: &Client,
    bucket: &str,
    key: &str,
    ttl: Duration,
) -> Result<String, R2Error> {
    let presign = aws_sdk_s3::presigning::PresigningConfig::expires_in(ttl)
        .map_err(|e| R2Error::Other(anyhow::anyhow!("presign config: {}", e)))?;
    let req = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .presigned(presign)
        .await
        .map_err(|e| R2Error::Other(anyhow::anyhow!("presign get: {}", e.into_service_error())))?;
    Ok(req.uri().to_string())
}

/// Pre-sign a PUT URL valid for `ttl`. Worker uploads the new wiki
/// tarball with this — no permanent credentials in the worker.
pub async fn presigned_put(
    client: &Client,
    bucket: &str,
    key: &str,
    ttl: Duration,
) -> Result<String, R2Error> {
    let presign = aws_sdk_s3::presigning::PresigningConfig::expires_in(ttl)
        .map_err(|e| R2Error::Other(anyhow::anyhow!("presign config: {}", e)))?;
    let req = client
        .put_object()
        .bucket(bucket)
        .key(key)
        .presigned(presign)
        .await
        .map_err(|e| R2Error::Other(anyhow::anyhow!("presign put: {}", e.into_service_error())))?;
    Ok(req.uri().to_string())
}

/// Upload raw bytes (api-side helper for small writes — wiki tarballs
/// go through pre-signed URLs from the worker, not this path).
pub async fn put_object(
    client: &Client,
    bucket: &str,
    key: &str,
    body: Vec<u8>,
    content_type: &str,
) -> Result<(), R2Error> {
    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(ByteStream::from(body))
        .content_type(content_type)
        .send()
        .await
        .map_err(|e| R2Error::Other(anyhow::anyhow!("put_object: {}", e.into_service_error())))?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum R2Error {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}
