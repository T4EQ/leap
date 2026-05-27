//! S3 backend implementation for downloading resources.
//!
//! This module provides [`S3Backend`], which implements the [`crate::downloader::backend::Backend`]
//! trait for interacting with Amazon S3 or S3-compatible storage services.
//! It supports downloading resources via URIs and fetching manifests.
//!
//! It depends on [`aws_sdk_s3`] for interacting with S3.

use std::pin::Pin;

use crate::cfg::S3Config;
use crate::downloader::Error;
use crate::downloader::backend::{Backend, ChunkResult};

use async_stream::stream;
use aws_sdk_s3::Client;
use secrecy::{ExposeSecret, SecretString};
use tokio_stream::Stream;

#[derive(Debug, Clone)]
struct ResolvedS3Config {
    pub endpoint_url: Option<(String, bool)>,
    pub access_key_id: SecretString,
    pub secret_access_key: SecretString,
    pub region: String,
}

/// An S3 backend for downloading resources.
///
/// This backend implements [`crate::downloader::backend::Backend`] and interacts with an
/// S3-compatible storage service to fetch resources and manifests.
pub struct S3Backend {
    client: Client,
    bucket: String,
}

impl S3Backend {
    fn resolve_s3_config(s3_config: &S3Config) -> anyhow::Result<ResolvedS3Config> {
        let endpoint_url = std::env::var("AWS_ENDPOINT_URL")
            .ok()
            .or(s3_config.endpoint_url.clone())
            .map(|e| (e, s3_config.force_path_style));

        let access_key_id = std::env::var("AWS_ACCESS_KEY_ID")
            .ok()
            .map(SecretString::from)
            .or(s3_config.access_key_id.clone())
            .ok_or(anyhow::anyhow!(concat!(
                "No AWS access key ID provided. ",
                "Please set either the AWS_ACCESS_KEY_ID environment variable or use ",
                "the LEAP configuration file to define the access_key_id"
            )))?;

        let secret_access_key = std::env::var("AWS_SECRET_ACCESS_KEY")
            .ok()
            .map(SecretString::from)
            .or(s3_config.secret_access_key.clone())
            .ok_or(anyhow::anyhow!(concat!(
                "No AWS secret access key provided. ",
                "Please set either the AWS_SECRET_ACCESS_KEY environment variable or use ",
                "the LEAP configuration file to define the secret_access_key"
            )))?;

        let region = std::env::var("AWS_REGION")
            .ok()
            .unwrap_or(s3_config.region.clone());

        Ok(ResolvedS3Config {
            endpoint_url,
            access_key_id,
            secret_access_key,
            region,
        })
    }

    /// Creates a new `S3Backend` for the given bucket.
    ///
    /// # Arguments
    ///
    /// * `bucket` - The name of the S3 bucket to use.
    /// * `s3_config` - The configuration for the S3 backend.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The S3 configuration cannot be resolved (e.g., missing credentials).
    /// - The AWS client fails to initialize.
    pub async fn new(bucket: &str, s3_config: &crate::cfg::S3Config) -> anyhow::Result<Self> {
        tracing::info!("Initializing S3 backend for bucket: {}", bucket);
        let s3_config = Self::resolve_s3_config(s3_config)?;
        tracing::debug!("✓ Using S3 configuration: {s3_config:?}");

        // Build AWS config with explicit credentials
        let creds = aws_sdk_s3::config::Credentials::new(
            s3_config.access_key_id.expose_secret(),
            s3_config.secret_access_key.expose_secret(),
            None,
            None,
            "config-file",
        );

        let retry_config = aws_config::retry::RetryConfig::standard().with_max_attempts(3);

        let config_loader = aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .credentials_provider(creds)
            .region(aws_sdk_s3::config::Region::new(s3_config.region))
            .retry_config(retry_config);

        let config = if let Some((endpoint_url, force_path_style)) = s3_config.endpoint_url {
            config_loader
                .endpoint_url(endpoint_url)
                .force_path_style(force_path_style)
                .build()
        } else {
            config_loader.build()
        };

        let client = Client::from_conf(config);

        // Note that at this point we do not validate the credentials because that was supposed to
        // be done during provisioning. Once the system is provisioned, we assume that the
        // credentials are correct, because validating them here might imply failing due to
        // spurious network errors (to which we need to be resilient due to the nature of the
        // environment where the project will run).

        Ok(Self {
            client,
            bucket: bucket.to_string(),
        })
    }

    /// Checks that we can access the bucket using the given credentials. This might be used, for
    /// instance, to check that we have access to the bucket after the user has provisioned the
    /// system with the given credentials.
    pub async fn verify_bucket_access(&self) -> anyhow::Result<()> {
        // Verify bucket access
        self.client
            .head_bucket()
            .bucket(&self.bucket)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("✗ Failed to verify S3 bucket access: {}", e);
                anyhow::anyhow!("Cannot access S3 bucket '{}': {}", self.bucket, e)
            })?;

        tracing::info!(
            "✓ Successfully verified access to S3 bucket: {}",
            self.bucket
        );
        Ok(())
    }

    async fn get_s3_object(
        &self,
        key: &str,
    ) -> Result<aws_sdk_s3::operation::get_object::GetObjectOutput, Error> {
        tracing::debug!("Fetching S3 object: s3://{}/{}", self.bucket, key);

        self.client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(
                    concat!(
                        "Failed to get S3 object s3://{}/{}: {}\n",
                        "Possible reasons:\n",
                        "  - File does not exist in S3\n",
                        "  - Missing s3:GetObject permission\n",
                        "  - Invalid AWS credentials\n",
                        "  - Network connectivity issue\n",
                    ),
                    self.bucket,
                    key,
                    e
                );
                Error::IoError(std::io::Error::other(format!(
                    "Failed to get S3 object s3://{}/{}: {}",
                    self.bucket, key, e
                )))
            })
    }
}

#[async_trait::async_trait]
impl Backend for S3Backend {
    fn fetch_resource<'a, 'b>(
        &'a self,
        uri: &'b http::Uri,
    ) -> Pin<Box<dyn Stream<Item = ChunkResult> + Send + 'a>>
    where
        'b: 'a,
    {
        Box::pin(stream! {
            let key = uri.path().trim_start_matches('/');

            let object = match self.get_s3_object(key).await {
                Ok(obj) => {
                    tracing::info!("Successfully initiated download of s3://{}/{}", self.bucket, key);
                    obj
                }
                Err(e) => {
                    yield Err(e);
                    return;
                }
            };

            let mut body = object.body;

            loop {
                match body.next().await {
                    Some(Ok(bytes)) => {
                        yield Ok(bytes.to_vec());
                    }
                    Some(Err(e)) => {
                        tracing::error!("Error reading S3 stream for s3://{}/{}: {}", self.bucket, key, e);
                        yield Err(Error::IoError(std::io::Error::other(
                            format!("Error reading S3 stream: {}", e)
                        )));
                        return;
                    }
                    None => {
                        tracing::debug!("Completed download of s3://{}/{}", self.bucket, key);
                        break;
                    }
                }
            }
        })
    }

    async fn fetch_manifest(&self) -> Result<Vec<u8>, Error> {
        tracing::info!("Fetching manifest from s3://{}/manifest.json", self.bucket);

        let result = self.get_s3_object("manifest.json").await?;

        let data = result.body.collect().await.map_err(|e| {
            tracing::error!("Failed to read manifest body: {}", e);
            Error::IoError(std::io::Error::other(format!(
                "Failed to read manifest body: {}",
                e
            )))
        })?;

        tracing::info!("Successfully fetched manifest from S3");
        Ok(data.into_bytes().to_vec())
    }
}
