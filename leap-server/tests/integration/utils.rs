//! Several reusable utilities for integration tests, including:
//! - A way to construct application configuration for isolated tests with
//!   guaranteed randomized directories.

use googletest::OrFail as _;
use http::Uri;
use leap_server::cfg::{DbConfig, DownloaderConfig, LeapConfig, RetryParams, S3Config};
use std::{net::TcpListener, path::PathBuf, str::FromStr as _, time::Duration};
use tokio::task::AbortHandle;

/// Constructs required filesystem resources to run an integration test.
/// Performs cleanup of the filesystem resources on drop.
pub struct TestResources {
    test_path: PathBuf,
}

impl TestResources {
    pub fn try_new_for_test() -> googletest::Result<Self> {
        let tmp_dir = std::env::temp_dir();
        let rand_path: String = (0..=10)
            .into_iter()
            .map(|_| rand::random_range('a'..='z'))
            .collect();
        let test_path = tmp_dir.join(rand_path);
        let resources = Self { test_path };
        std::fs::create_dir_all(resources.content_path()).or_fail()?;
        std::fs::create_dir_all(resources.runtime_path()).or_fail()?;
        std::fs::create_dir_all(resources.file_server_path()).or_fail()?;
        Ok(resources)
    }

    pub fn content_path(&self) -> PathBuf {
        self.test_path.join("content_path")
    }

    pub fn runtime_path(&self) -> PathBuf {
        self.test_path.join("runtime_path")
    }

    pub fn file_server_path(&self) -> PathBuf {
        self.test_path.join("file_server_path")
    }

    pub fn leap_config(&self) -> googletest::Result<LeapConfig> {
        let uri = &format!("file:/{}", self.file_server_path().display());
        Ok(LeapConfig {
            debug: false,
            downloader_config: DownloaderConfig {
                concurrent_downloads: 1,
                content_path: self.content_path(),
                remote_server: Uri::from_str(uri).or_fail()?,
                // By default no automatic update of the manifest, leave it up to the test to trigger
                // updates.
                update_interval: Duration::MAX,
                // No retries by default.
                retry_params: RetryParams::fixed_backoff(Duration::MAX),
            },
            db_config: DbConfig {
                busy_timeout: Duration::from_secs(10),
                pool_size: 16,
                runtime_path: self.runtime_path(),
            },
            s3_config: S3Config::default(),
        })
    }
}

impl Drop for TestResources {
    fn drop(&mut self) {
        // Best effort removal
        let _ = std::fs::remove_dir_all(&self.test_path);
    }
}

pub struct TestServer {
    port: u16,
    handle: AbortHandle,
}

impl TestServer {
    pub fn start(test_resources: &TestResources) -> googletest::Result<Self> {
        // let's try to find a random unused port
        let (listener, port) = {
            loop {
                let port: u16 = rand::random_range(30000..32000);
                let addr = format!("localhost:{port}");
                match TcpListener::bind(&addr) {
                    Ok(listener) => break (listener, port),
                    Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {}
                    Err(error) => return Err(error.into()),
                }
            }
        };

        let leap_config = test_resources.leap_config().or_fail()?;
        let handle = tokio::spawn(leap_server::run_app(listener, leap_config));
        let handle = handle.abort_handle();
        Ok(Self { port, handle })
    }

    pub fn endpoint_url(&self, path: &str) -> String {
        format!("http://localhost:{}/{}", self.port, path)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
