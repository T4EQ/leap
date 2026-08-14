//! Several reusable utilities for integration tests, including:
//! - A way to construct application configuration for isolated tests with
//!   guaranteed randomized directories.

use googletest::prelude::*;
use http::Uri;
use leap_server::{
    cfg::{DbConfig, DownloaderConfig, LeapConfig, RetryParams, S3Config},
    manifest::{self},
};
use sha2::Digest;
use std::{net::TcpListener, path::PathBuf, str::FromStr as _, time::Duration};
use tokio::task::AbortHandle;
use uuid::Uuid;

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
        let uri = &format!("file://localhost{}", self.file_server_path().display());
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

    pub async fn save_manifest_to_remote_server(
        &self,
        date: chrono::NaiveDate,
        videos: &[(String, Vec<manifest::Video>)],
    ) -> googletest::Result<()> {
        let manifest = manifest::ManifestFile {
            name: "manifest_file".to_owned(),
            date,
            version: manifest::Version {
                major: 1,
                minor: 0,
                revision: 0,
            },
            sections: videos
                .iter()
                .map(|(name, vids)| manifest::Section {
                    name: name.to_owned(),
                    content: vids.clone(),
                })
                .collect(),
        };

        let manifest_data = serde_json::to_vec(&manifest).unwrap();
        tokio::fs::write(self.file_server_path().join("manifest.json"), manifest_data).await?;
        Ok(())
    }

    pub async fn save_video_file_to_remote_server(
        &self,
        name: &str,
        length: usize,
    ) -> googletest::Result<(manifest::Video, Vec<u8>)> {
        let data: Vec<u8> = rand::random_iter().take(length).collect();
        let id = Uuid::new_v4();
        let file_path = self.file_server_path().join(id.to_string());
        let uri = &format!("file://localhost/{id}");
        let mut sha256_gen = sha2::Sha256::new();
        sha256_gen.update(&data);
        let sha256 = sha256_gen.finalize().to_vec();
        let sha256 = manifest::Sha256::try_from(sha256.as_slice()).or_fail()?;
        let video = manifest::Video {
            id,
            name: name.to_owned(),
            file_size: data.len().try_into().or_fail()?,
            uri: Uri::from_str(uri).or_fail()?,
            sha256,
        };

        tokio::fs::write(file_path, &data).await?;
        Ok((video, data))
    }

    /// Takes an array of sections, containing name and an array of videos, containing name and length.
    /// Saves the videos to the remote file server and publishes a manifest with them.
    pub async fn save_videos_and_publish_manifest(
        &self,
        date: chrono::NaiveDate,
        sections: &[(String, Vec<(String, usize)>)],
    ) -> googletest::Result<Vec<(manifest::Video, Vec<u8>)>> {
        let mut all_videos = vec![];
        let mut manifest_sections = vec![];
        for (name, content) in sections {
            let mut videos = vec![];
            for (name, size) in content {
                let (video, data) = self
                    .save_video_file_to_remote_server(name, *size)
                    .await
                    .or_fail()?;
                all_videos.push((video.clone(), data));
                videos.push(video);
            }
            manifest_sections.push(((*name).to_owned(), videos));
        }

        self.save_manifest_to_remote_server(date, &manifest_sections)
            .await
            .or_fail()?;
        Ok(all_videos)
    }

    pub async fn verify_saved_video_matches(
        &self,
        video: &manifest::Video,
        expected_data: &[u8],
    ) -> googletest::Result<()> {
        let path = self.content_path().join(format!("{}.mp4", video.id));
        let data = tokio::fs::read(path).await.or_fail()?;
        verify_that!(data, container_eq(expected_data.to_vec()))?;
        Ok(())
    }
}

impl Drop for TestResources {
    fn drop(&mut self) {
        // Best effort removal
        let _ = std::fs::remove_dir_all(&self.test_path);
    }
}

pub struct TestServer<'a> {
    // Resources cannot be destroyed while the server operates.
    _test_resources: std::marker::PhantomData<&'a TestResources>,
    port: u16,
    handle: AbortHandle,
}

impl<'a> TestServer<'a> {
    pub fn start(test_resources: &'a TestResources) -> googletest::Result<Self> {
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
        Ok(Self {
            _test_resources: std::marker::PhantomData,
            port,
            handle,
        })
    }

    pub fn endpoint_url(&self, path: &str) -> String {
        format!("http://localhost:{}/{}", self.port, path)
    }
}

impl<'a> Drop for TestServer<'a> {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

pub async fn await_video_downloads(
    server: &TestServer<'_>,
    videos: impl IntoIterator<Item = &manifest::Video> + Clone,
    timeout: Duration,
) -> googletest::Result<()> {
    let url = server.endpoint_url("api/content/events");

    let mut response = reqwest::get(url).await.or_fail()?;
    let operation = async {
        loop {
            let chunk = response.chunk().await.or_fail()?.or_fail()?;
            if chunk.starts_with(b"data: ") {
                let chunk = &chunk[6..];
                let parsed: leap_api::api::content::meta::get::Response =
                    serde_json::from_slice(chunk).or_fail()?;
                let Some(meta) = parsed.meta else {
                    continue;
                };

                let content_iter = meta.content.into_iter().flat_map(|s| s.content);

                let all_downloaded = videos.clone().into_iter().all(|v| {
                    let video_id = v.id.to_string();
                    content_iter
                        .clone()
                        .find(|candidate| {
                            candidate.id == video_id
                                && candidate.status
                                    == leap_api::api::content::meta::get::VideoStatus::Downloaded
                        })
                        .is_some()
                });

                if all_downloaded {
                    break Ok(());
                }
            }
        }
    };

    tokio::time::timeout(timeout, operation).await.or_fail()?
}
