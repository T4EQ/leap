//! Tests of the LEAP configuration module

use googletest::OrFail as _;
use googletest::prelude::*;
use http::Uri;
use leap_server::cfg::DbConfig;
use leap_server::cfg::DownloaderConfig;
use leap_server::cfg::LeapConfig;
use leap_server::cfg::RetryParams;
use leap_server::cfg::S3Config;
use secrecy::ExposeSecret as _;
use std::ffi::OsString;
use std::sync::LazyLock;
use std::time::Duration;
use std::{path::PathBuf, str::FromStr as _};

static EXPECTED_CONFIG: LazyLock<LeapConfig> = LazyLock::new(|| LeapConfig {
    debug: true,
    downloader_config: DownloaderConfig {
        concurrent_downloads: 8,
        content_path: PathBuf::from_str("/tmp/leap/content_path").unwrap(),
        remote_server: Uri::from_static("s3://bucket"),
        update_interval: Duration::from_secs(20),
        retry_params: RetryParams {
            initial_backoff: Duration::from_secs(5),
            backoff_factor: 1.5,
            max_backoff: Duration::from_hours(2),
        },
    },
    db_config: DbConfig {
        busy_timeout: Duration::from_secs(10),
        pool_size: 16,
        runtime_path: PathBuf::from_str("/tmp/leap/runtime_path").unwrap(),
    },
    s3_config: S3Config {
        endpoint_url: Some("https://s3.server.com".to_owned()),
        access_key_id: Some("1234".into()),
        secret_access_key: Some("4567".into()),
        force_path_style: true,
        region: "us-east-1".to_owned(),
    },
});

#[rstest::rstest]
#[case(
    vec![],
    || {
        EXPECTED_CONFIG.clone()
    }
)]
#[case(
    vec![("LEAP_DEBUG", "false")],
    || {
        let mut cfg = EXPECTED_CONFIG.clone();
        cfg.debug = false;
        cfg
    }
)]
#[case(
    vec![("LEAP_DOWNLOADER_CONFIG__CONCURRENT_DOWNLOADS", "32")],
    || {
        let mut cfg = EXPECTED_CONFIG.clone();
        cfg.downloader_config.concurrent_downloads = 32;
        cfg
    }
)]
#[case(
    vec![("LEAP_DOWNLOADER_CONFIG__CONTENT_PATH", "new_content_path")],
    || {
        let mut cfg = EXPECTED_CONFIG.clone();
        cfg.downloader_config.content_path = PathBuf::from_str("new_content_path").unwrap();
        cfg
    }
)]
#[case(
    vec![
        ("LEAP_S3_CONFIG__ACCESS_KEY_ID", "access_key_id"),
        ("LEAP_DB_CONFIG__BUSY_TIMEOUT", "1 sec")
    ],
    || {
        let mut cfg = EXPECTED_CONFIG.clone();
        cfg.s3_config.access_key_id = Some("access_key_id".into());
        cfg.db_config.busy_timeout = Duration::from_secs(1);
        cfg
    }
)]
#[gtest]
fn overrides_with_env_vars(
    #[case] env_vars: Vec<(&str, &str)>,
    #[case] expected_config_generator: fn() -> LeapConfig,
) -> googletest::Result<()> {
    let expected_config = expected_config_generator();
    let env_vars: Vec<(OsString, Option<OsString>)> = env_vars
        .into_iter()
        .map(|(name, val)| (name.into(), Some(val.into())))
        .collect();

    let path = PathBuf::from_str(concat!(
        std::env!("CARGO_MANIFEST_DIR"),
        "/tests/example_config.toml"
    ))
    .or_fail()?;
    let config = temp_env::with_vars(env_vars, || leap_server::cfg::get_config(&path)).or_fail()?;

    expect_eq!(config.debug, expected_config.debug);
    expect_eq!(
        config.downloader_config.concurrent_downloads,
        expected_config.downloader_config.concurrent_downloads
    );
    expect_eq!(
        config.downloader_config.content_path,
        expected_config.downloader_config.content_path
    );
    expect_eq!(
        config.downloader_config.remote_server,
        expected_config.downloader_config.remote_server
    );
    expect_eq!(
        config.downloader_config.update_interval,
        expected_config.downloader_config.update_interval
    );
    expect_eq!(
        config.downloader_config.retry_params.initial_backoff,
        expected_config
            .downloader_config
            .retry_params
            .initial_backoff
    );
    expect_eq!(
        config.downloader_config.retry_params.backoff_factor,
        expected_config
            .downloader_config
            .retry_params
            .backoff_factor
    );
    expect_eq!(
        config.downloader_config.retry_params.max_backoff,
        expected_config.downloader_config.retry_params.max_backoff
    );

    expect_eq!(
        config.db_config.pool_size,
        expected_config.db_config.pool_size
    );
    expect_eq!(
        config.db_config.busy_timeout,
        expected_config.db_config.busy_timeout
    );
    expect_eq!(
        config.db_config.runtime_path,
        expected_config.db_config.runtime_path
    );

    expect_eq!(
        config.s3_config.endpoint_url,
        expected_config.s3_config.endpoint_url,
    );
    expect_eq!(
        config.s3_config.force_path_style,
        expected_config.s3_config.force_path_style
    );
    expect_eq!(
        config
            .s3_config
            .access_key_id
            .map(|s| s.expose_secret().to_owned()),
        expected_config
            .s3_config
            .access_key_id
            .clone()
            .map(|s| s.expose_secret().to_owned())
    );
    expect_eq!(
        config
            .s3_config
            .secret_access_key
            .map(|s| s.expose_secret().to_owned()),
        expected_config
            .s3_config
            .secret_access_key
            .clone()
            .map(|s| s.expose_secret().to_owned()),
    );
    expect_eq!(config.s3_config.region, expected_config.s3_config.region);

    Ok(())
}
