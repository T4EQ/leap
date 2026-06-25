//! Provision module for the LEAP configuration file. Exposes [`provision_configuration`], which
//! receives external leap configuration and saves it to disk after validation.

use std::time::Duration;

use super::{CONTENT_PATH, MOUNT_PATH, RUNTIME_PATH};
use crate::{
    cfg::{DEFAULT_CONFIG_PATH, DbConfig, DownloaderConfig, LeapConfig, RetryParams, S3Config},
    utils,
};

impl From<&leap_api::provision::config::post::LeapConfig> for LeapConfig {
    fn from(value: &leap_api::provision::config::post::LeapConfig) -> Self {
        let has_custom_endpoint = value.s3_config.endpoint_url.is_some();
        Self {
            debug: false,
            db_config: DbConfig {
                // These parameters are not considered to be user-configurable.
                busy_timeout: Duration::from_secs(10),
                pool_size: 16,
                runtime_path: RUNTIME_PATH.into(),
            },
            s3_config: S3Config {
                endpoint_url: value.s3_config.endpoint_url.clone(),
                force_path_style: value
                    .s3_config
                    .force_path_style
                    .unwrap_or(has_custom_endpoint),
                access_key_id: Some(value.s3_config.access_key_id.clone()),
                secret_access_key: Some(value.s3_config.secret_access_key.clone()),
                region: value
                    .s3_config
                    .region
                    .as_deref()
                    .unwrap_or("us-east-1")
                    .to_owned(),
            },
            downloader_config: DownloaderConfig {
                concurrent_downloads: value.downloader_config.concurrent_downloads,
                remote_server: value.s3_config.bucket.clone(),
                update_interval: value.downloader_config.update_interval,
                content_path: CONTENT_PATH.into(),
                retry_params: RetryParams {
                    initial_backoff: value.downloader_config.retry_params.initial_backoff,
                    backoff_factor: value.downloader_config.retry_params.backoff_factor,
                    max_backoff: value.downloader_config.retry_params.max_backoff,
                },
            },
        }
    }
}

/// Saves LEAP configuration to disk. Ensures that the S3 credentials are valid by enabling
/// temporarily the network connection, then waiting for time sync (NTP) to ensure HTTPs will work,
/// and finally by checking connectivity to S3. The provision network configuration is restored
/// before exiting. The configuration is only persisted if validation succeeds.
pub async fn provision_configuration(
    config: &leap_api::provision::config::post::LeapConfig,
) -> anyhow::Result<LeapConfig> {
    tracing::info!("Checking that the block device is mounted.");
    let blockdevs = super::storage::list_blockdevs().await?;
    let mounted = blockdevs
        .iter()
        .any(|b| b.mountpoints.contains(&MOUNT_PATH.to_owned()));
    if !mounted {
        tracing::error!("Block device is not mounted at {MOUNT_PATH}");
        anyhow::bail!("Cannot apply configuration. {MOUNT_PATH} is not mounted");
    }
    tracing::info!("Block device is mounted");

    let config: LeapConfig = config.into();

    // Check S3 access to validate configuration. For this, we need to temporarily enable the
    // network setup and then come back to the original setup network.
    {
        let config = config.clone();
        super::network::temporarily_enable_network_config(async move || -> anyhow::Result<()> {
            // Wait for time sync, since HTTPs certs for S3 require our time to be somewhat close
            // to the reality, and we do not have an RTC on the board.
            tracing::info!("Waiting for time sync");
            const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
            // This is a best-effort synchronization. It might be that date was previously
            // synchronized, so we try to connect to S3 either way.
            let _ = utils::wait_timesync(TIMEOUT)
                .await
                .inspect(|_| tracing::info!("Time synchronized"))
                .inspect_err(|e| tracing::error!("Time synchronization failed: {e:?}"));

            let bucket = config
                .downloader_config
                .remote_server
                .host()
                .ok_or_else(|| {
                    tracing::error!("Invalid S3 URI");
                    anyhow::anyhow!("S3 URI must specify a bucket name")
                })?;
            tracing::info!("Checking access to bucket.");
            let s3_backend =
                crate::downloader::s3backend::S3Backend::new(bucket, &config.s3_config).await?;
            s3_backend.verify_bucket_access().await.inspect_err(|e| {
                tracing::error!("Bucket access failed: {e}");
            })?;
            tracing::info!("Bucket access Ok");
            Ok(())
        })
        .await?;
    }

    // Save configuration to file
    tracing::info!("Saving configuration to file");
    let target_dir: std::path::PathBuf = DEFAULT_CONFIG_PATH.into();
    if let Some(parent) = target_dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let serialized_config = toml::to_string(&config)?;
    tokio::fs::write(target_dir, serialized_config.as_bytes()).await?;
    tracing::info!("Configuration saved.");

    Ok(config)
}
