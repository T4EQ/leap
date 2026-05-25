//! This module provides the core logic for downloading and managing video content based on manifests.
//!
//! It handles the entire lifecycle of a download:
//! - Initializing video entries in the database from a new manifest.
//! - Cleaning up old video content that is no longer present in the manifest.
//! - Managing a pool of concurrent download tasks with retry logic and exponential backoff.
//! - Verifying the integrity of downloaded files using SHA-256 checksums.
//!
//! It depends on [`crate::db::Database`] for persistence and [`crate::manifest::ManifestFile`] for
//! describing the content to be downloaded.
//!
//! # Usage Guide
//!
//! The primary entry point for downloading a manifest is [`download_manifest_task`].

use crate::{
    db::{Database, DownloadStatus},
    manifest::{ManifestFile, Video},
};

use super::DownloadContext;

use std::collections::VecDeque;

use sha2::Digest;
use tokio::{io::AsyncWriteExt, task::JoinSet};
use tokio_stream::StreamExt;

/// Makes sure that all manifest videos are present in the database with their corresponding state.
/// Creates entries for missing videos.
#[tracing::instrument(name = "initialize_video_entries", skip(database, new_manifest))]
pub async fn initialize_video_entries(
    database: &Database,
    new_manifest: &ManifestFile,
) -> anyhow::Result<()> {
    for video in new_manifest.sections.iter().flat_map(|s| s.content.iter()) {
        match database.find_video(video.id).await {
            Ok(_) => {}
            Err(crate::db::Error::Diesel(diesel::result::Error::NotFound)) => {
                database
                    .insert_video(video.id, &video.name, video.file_size)
                    .await?
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

#[tracing::instrument(name = "publish_manifest", skip(db, new_manifest), fields(manifest_date = %new_manifest.date))]
pub async fn publish_manifest(db: &Database, new_manifest: &ManifestFile) {
    db.publish_manifest(new_manifest).await;
}

/// Iterates through the on-disk video entries, deleting video content that is not present in the current
/// manifest. This is a cleanup action that is deferred until the new manifest has been fully
/// adopted.
#[tracing::instrument(name = "remove_old_video_content", skip(database, new_manifest))]
pub async fn remove_old_video_content(
    content_path: &std::path::Path,
    database: &Database,
    new_manifest: &ManifestFile,
) -> anyhow::Result<()> {
    let in_manifest = |id| {
        new_manifest
            .sections
            .iter()
            .flat_map(|s| s.content.iter())
            .any(|v| v.id == id)
    };

    for video in database.list_all_videos().await? {
        if !in_manifest(video.id) {
            database.delete_video(video.id).await?;
            if let DownloadStatus::Downloaded(path) = video.download_status {
                tokio::fs::remove_file(path).await?;
            } else {
                // Try to remove it from the current runtime_path. Not only fully downloaded videos
                // need to be deleted.
                let path = content_path.join(format!("{}.mp4", video.id));
                // The file might already not exist, if the download never started. Therefore we
                // don't error out and do best effort deletion here.
                let _ = tokio::fs::remove_file(path).await;
            }
        }
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct Job {
    backoff_time: std::time::Duration,
    video: Video,
}

/// When the leap-server command is interrupted, downloads that might have been previously in
/// progress are now lost. In order to more clearly report the download state after a reboot of the
/// server, we mark them as failed with a corresponding reason, instead of saying they are
/// downloading with some fake progress.
#[tracing::instrument(
    name = "mark_interrupted_downloads",
    skip(database, manifest),
    fields(manifest_date = %manifest.date)
)]
pub async fn mark_interrupted_downloads(
    database: &Database,
    manifest: &ManifestFile,
) -> anyhow::Result<()> {
    for video in manifest.sections.iter().flat_map(|s| s.content.iter()) {
        match database.find_video(video.id).await {
            Ok(crate::db::Video {
                download_status: crate::db::DownloadStatus::InProgress(_),
                ..
            }) => {
                database
                    .set_download_failed(video.id, "Download interrupted due to system restart")
                    .await?;
            }
            Ok(_) | Err(crate::db::Error::Diesel(diesel::result::Error::NotFound)) => {
                // Nothing to do, these are handled separately when starting to fetch
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// An async task in charge of downloading the content listed in a manifest.
///
/// This task needs to be cancel-safe, because it might get cancelled by calling code if a newer
/// manifest is found.
/// For references on cancellation-safety: https://sunshowers.io/posts/cancelling-async-rust/
#[tracing::instrument(
    name = "download_manifest_task",
    skip(ctx, new_manifest),
    fields(manifest_date = %new_manifest.date)
)]
pub async fn download_manifest_task(
    ctx: DownloadContext,
    new_manifest: ManifestFile,
) -> anyhow::Result<()> {
    initialize_video_entries(&ctx.db, &new_manifest).await?;

    // After the video entries for the current manifest have been populated, we are ready to
    // publish the manifest and make it visible to the HTTP clients.
    publish_manifest(&ctx.db, &new_manifest).await;

    // Mark older content for deletion
    remove_old_video_content(&ctx.config.content_path, &ctx.db, &new_manifest).await?;

    // Collect the content that we need to download
    let mut pending_downloads: VecDeque<Job> = VecDeque::new();
    for video in new_manifest.sections.iter().flat_map(|s| s.content.iter()) {
        let already_downloaded = ctx
            .db
            .find_video(video.id)
            .await
            .unwrap()
            .download_status
            .is_downloaded();
        if pending_downloads.iter().all(|j| video.id != j.video.id) && !already_downloaded {
            pending_downloads.push_back(Job {
                video: video.clone(),
                backoff_time: ctx.config.retry_params.initial_backoff,
            });
        }
    }

    tracing::debug!("Videos pending download: {pending_downloads:?}");

    // Because we do not want to ovewhelm the network, we limit the number of concurrent downloads
    // we perform. This limit is configurable via the configuration file.
    let mut inprogress_videos = JoinSet::new();
    let mut backoff_list = VecDeque::new();

    loop {
        if inprogress_videos.is_empty() && backoff_list.is_empty() && pending_downloads.is_empty() {
            break;
        }

        // Try to start more downloads while we have some
        while inprogress_videos.len() < ctx.config.concurrent_downloads {
            let Some(current_job) = pending_downloads.pop_front() else {
                break;
            };

            let job = download_job_task(ctx.clone(), current_job.clone());
            inprogress_videos.spawn(job);
        }

        // We have 2 situations to wait for here.
        //  1. A download finished, which opens up a new slot to start another download
        //  2. A failed video which was held has now completed the backoff duration and can be
        //     scheduled again.
        let first_backoff_video = async {
            let Some(wakeup_time) = backoff_list
                .iter()
                .next()
                .map(|(wakeup_time, _)| *wakeup_time)
            else {
                // Make sure this does not exit unless we actually need to do work
                std::future::pending().await
            };

            tokio::time::sleep_until(wakeup_time).await;

            // Actually finished waiting, so let's pop it off now (to be cancel-safe).
            let (_, job): (tokio::time::Instant, Job) = backoff_list.pop_front().expect("");
            job
        };

        tokio::select! {
            job = first_backoff_video => {
                tracing::info!("Video {} will reattempt download", job.video.id);
                pending_downloads.push_back(job);
            }

            Some(finished_video) = inprogress_videos.join_next() => {
                match finished_video? {
                    Ok(()) => { }
                    Err(DownloadJobError::ShouldRetry(mut job)) => {
                        tracing::error!("Video {} failed. Backing off for {:?}", job.video.id, job.backoff_time);
                        let wakeup_time = tokio::time::Instant::now() + job.backoff_time;
                        job.backoff_time = job.backoff_time .mul_f64( ctx.config.retry_params.backoff_factor);
                        backoff_list.push_back((wakeup_time, *job));
                    }
                    Err(DownloadJobError::Unrecoverable(job)) => {
                        let msg = format!("Unrecoverable download error for video: {}", job.video.id);
                        tracing::error!(msg);
                        anyhow::bail!(msg);
                    }
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
enum DownloadJobError {
    ShouldRetry(Box<Job>),
    Unrecoverable(Box<Job>),
}

impl DownloadJobError {
    #[cfg(test)]
    fn to_job(&self) -> Job {
        match self {
            DownloadJobError::ShouldRetry(job) => (**job).clone(),
            DownloadJobError::Unrecoverable(job) => (**job).clone(),
        }
    }
}

/// download job task
#[tracing::instrument(
    name = "download_job_task",
    skip(ctx, job),
    fields(
        video_id = %job.video.id,
    )
)]
async fn download_job_task(ctx: DownloadContext, job: Job) -> Result<(), DownloadJobError> {
    let video = &job.video;
    let mut stream = ctx.backend.fetch_resource(&video.uri);

    let target_filepath = ctx.config.content_path.join(format!("{}.mp4", video.id));
    if let Some(dir) = target_filepath.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| {
            tracing::error!("Error creating directory: {dir:?}. Error: {e}");
            DownloadJobError::ShouldRetry(Box::new(job.clone()))
        })?;
    }
    let mut target_file = tokio::fs::File::create(&target_filepath)
        .await
        .map_err(|e| {
            tracing::error!("Error creating file: {target_filepath:?}. Error: {e}");
            DownloadJobError::ShouldRetry(Box::new(job.clone()))
        })?;

    let translate_error = |e: crate::db::Result<()>| {
        e.map_err(|e| {
            tracing::error!(
                "Error setting download status for file: {target_filepath:?}. Error: {e}",
            );
            DownloadJobError::Unrecoverable(Box::new(job.clone()))
        })
    };

    let mut hasher = sha2::Sha256::new();

    let mut total_size = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(v) => v,
            Err(err) => {
                let error_msg = format!(
                    "Error fetching file with id: {}, name: {}. path: {}. Error: {}.",
                    video.id, video.name, video.uri, err
                );
                tracing::error!("{error_msg}");

                translate_error(ctx.db.set_download_failed(video.id, &error_msg).await)?;

                return Err(DownloadJobError::ShouldRetry(Box::new(job.clone())));
            }
        };

        hasher.update(&chunk[..]);
        target_file.write_all(&chunk[..]).await.map_err(|e| {
            tracing::error!("Error writing file: {target_filepath:?}. Error: {e}");
            DownloadJobError::ShouldRetry(Box::new(job.clone()))
        })?;
        total_size += chunk.len();

        tracing::trace!(
            "Got chunk of {} bytes. Progress: {:.2} %",
            chunk.len(),
            (total_size as f64) / (job.video.file_size as f64) * 100.0
        );

        translate_error(
            ctx.db
                .update_download_progress(video.id, total_size as u64)
                .await,
        )?;
    }

    let hash = hasher.finalize();
    let hash = hash.as_slice();
    let expected_hash = video.sha256.as_bytes();
    if hash != &expected_hash[..] {
        let hash: crate::manifest::Sha256 = hash.try_into().expect("Should have 32 bytes");
        let err_msg = &format!("Got hash: {hash}. Expected: {}", video.sha256);
        translate_error(ctx.db.set_download_failed(video.id, err_msg).await)?;
        tracing::error!("{}", err_msg);
        return Err(DownloadJobError::ShouldRetry(Box::new(job.clone())));
    }

    translate_error(ctx.db.set_downloaded(video.id, &target_filepath).await)?;
    tracing::info!("Video downloaded successfully to: {target_filepath:?}");
    Ok(())
}

#[cfg(test)]
pub mod test {
    use std::{str::FromStr, sync::Arc, time::Duration};

    use crate::{
        cfg::{DbConfig, DownloaderConfig, RetryParams},
        downloader::backend::{self, Backend},
        manifest::{ManifestFile, Section, Version, Video},
    };

    use googletest::prelude::*;
    use http::Uri;

    use super::*;

    fn manifest_for_test() -> googletest::Result<ManifestFile> {
        Ok(ManifestFile {
            name: "manifest".to_string(),
            date: chrono::NaiveDate::from_str("2025-10-10").or_fail()?,
            version: Version {
                major: 2,
                minor: 0,
                revision: 0,
            },
            sections: vec![
                Section {
                    name: "".to_string(),
                    content: vec![
                        Video {
                            name: "Linear equations".to_string(),
                            id: uuid::Uuid::from_str("bf978778-1c5d-44b3-b2c1-1cc253563799")
                                .or_fail()?,
                            uri: "s3://bucket/linear-equations.mp4".parse().or_fail()?,
                            sha256:
                                "0b88b2dec2be5e2ef74022ef6a8023232e28374d67e917b76f9bb607e691f327"
                                    .try_into()
                                    .or_fail()?,
                            file_size: 123456,
                        },
                        Video {
                            name: "Quadratic equations".to_string(),
                            id: uuid::Uuid::from_str("5eb9e089-79cf-478d-9121-9ca3e7bb1d4a")
                                .or_fail()?,
                            uri: "s3://bucket/quadratic-equations.mp4".parse().or_fail()?,
                            sha256:
                                "8f9e3a4ae7d86c4abdf731a947fc90b607b82a0362da0b312e3b644defedb81f"
                                    .try_into()
                                    .or_fail()?,
                            file_size: 123457,
                        },
                    ],
                },
                Section {
                    name: "Integration".to_string(),
                    content: vec![
                        Video {
                            name: "Riemann sum".to_string(),
                            id: uuid::Uuid::from_str("eddb4450-a9ff-4a4b-ad81-2a8b78998405")
                                .or_fail()?,
                            uri: "s3://bucket/riemann-sum.mp4".parse().or_fail()?,
                            sha256:
                                "a6d3b80cd14f78b21ffbf5995bbda38ad8834459557782d245ed720134d36fc4"
                                    .try_into()
                                    .or_fail()?,
                            file_size: 123459,
                        },
                        Video {
                            name: "List of integrals".to_string(),
                            id: uuid::Uuid::from_str("f47e6cdc-1bcf-439a-9ea4-038dc7153648")
                                .or_fail()?,
                            uri: "s3://bucket/list-of-integrals.mp4".parse().or_fail()?,
                            sha256:
                                "98780990e94fb55d0b88ebcd78fe82f069eac547731a4b0822332d826c970aec"
                                    .try_into()
                                    .or_fail()?,
                            file_size: 123460,
                        },
                    ],
                },
            ],
        })
    }

    fn manifest_for_test2() -> googletest::Result<ManifestFile> {
        Ok(ManifestFile {
            name: "manifest 2".to_string(),
            date: chrono::NaiveDate::from_str("2025-10-11").or_fail()?,
            version: Version {
                major: 2,
                minor: 0,
                revision: 0,
            },
            sections: vec![Section {
                name: "Section with a name".to_string(),
                content: vec![
                    Video {
                        name: "Quadratic equations".to_string(),
                        id: uuid::Uuid::from_str("5eb9e089-79cf-478d-9121-9ca3e7bb1d4a")
                            .or_fail()?,
                        uri: "s3://bucket/quadratic-equations.mp4".parse().or_fail()?,
                        sha256: "8f9e3a4ae7d86c4abdf731a947fc90b607b82a0362da0b312e3b644defedb81f"
                            .try_into()
                            .or_fail()?,
                        file_size: 123457,
                    },
                    Video {
                        name: "Riemann sum".to_string(),
                        id: uuid::Uuid::from_str("eddb4450-a9ff-4a4b-ad81-2a8b78998405")
                            .or_fail()?,
                        uri: "s3://bucket/riemann-sum.mp4".parse().or_fail()?,
                        sha256: "a6d3b80cd14f78b21ffbf5995bbda38ad8834459557782d245ed720134d36fc4"
                            .try_into()
                            .or_fail()?,
                        file_size: 123459,
                    },
                ],
            }],
        })
    }

    struct TestContext {
        dummy_backend: Arc<DummyBackend>,
        download_ctx: DownloadContext,

        // We need to keep these to make sure the dirs are not removed from the fs
        _content_path: tempfile::TempDir,
        _runtime_path: tempfile::TempDir,
    }

    async fn create_context() -> TestContext {
        let content_path = tempfile::TempDir::new().unwrap();
        let downloader_config = Arc::new(DownloaderConfig {
            concurrent_downloads: 2,
            content_path: content_path.path().to_path_buf(),
            retry_params: RetryParams {
                initial_backoff: Duration::from_millis(100),
                backoff_factor: 1.0,
                max_backoff: Duration::from_millis(100),
            },
            remote_server: "/Invalid".try_into().unwrap(),
            update_interval: Duration::from_secs(300),
        });

        let runtime_path = tempfile::TempDir::new().unwrap();
        let db_config = DbConfig {
            busy_timeout: Duration::from_secs(2),
            runtime_path: runtime_path.path().to_path_buf(),
            pool_size: 16,
        };

        let db = Arc::new(Database::open(db_config).await.unwrap());
        db.apply_pending_migrations().await.or_fail().unwrap();

        let dummy_backend = Arc::new(DummyBackend::default());

        let download_ctx = DownloadContext {
            config: downloader_config,
            backend: dummy_backend.clone(),
            db,
        };

        TestContext {
            dummy_backend,
            download_ctx,
            _content_path: content_path,
            _runtime_path: runtime_path,
        }
    }

    struct BackendFile {
        uri: Uri,
        content: Vec<u8>,
    }

    struct DummyBackend {
        files: tokio::sync::Mutex<Vec<BackendFile>>,
    }

    impl Default for DummyBackend {
        fn default() -> Self {
            Self {
                files: tokio::sync::Mutex::new(vec![]),
            }
        }
    }

    impl DummyBackend {
        async fn add_file(&self, file: BackendFile) {
            let mut files = self.files.lock().await;
            files.push(file);
        }
    }

    #[async_trait::async_trait]
    impl Backend for DummyBackend {
        fn fetch_resource<'a, 'b>(
            &'a self,
            uri: &'b http::Uri,
        ) -> std::pin::Pin<Box<dyn tokio_stream::Stream<Item = backend::ChunkResult> + Send + 'a>>
        where
            'b: 'a,
        {
            Box::pin(async_stream::stream! {
                let files = self.files.lock().await;
                let Some(file) = files.iter().find(|f| f.uri == *uri) else {
                    yield Err(crate::downloader::Error::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "")));
                    return;
                };

                yield Ok(file.content.clone());
            })
        }

        async fn fetch_manifest(&self) -> std::result::Result<Vec<u8>, crate::downloader::Error> {
            // Not needed for these tests
            unimplemented!()
        }
    }

    #[tokio::test]
    #[googletest::test]
    async fn test_initialize_video_entries() -> googletest::Result<()> {
        let ctx = create_context().await;
        let db = &ctx.download_ctx.db;

        let manifest = manifest_for_test()?;

        initialize_video_entries(db, &manifest).await.or_fail()?;

        for video in manifest.sections.iter().flat_map(|s| s.content.iter()) {
            let db_video = db.find_video(video.id).await.or_fail()?;
            expect_that!(
                db_video,
                eq(&crate::db::Video {
                    id: video.id,
                    name: video.name.clone(),
                    file_size: video.file_size,
                    download_status: crate::db::DownloadStatus::Pending,
                    view_count: 0,
                })
            );
        }

        Ok(())
    }

    #[tokio::test]
    #[googletest::test]
    async fn test_remove_old_video_content() -> googletest::Result<()> {
        let ctx = create_context().await;
        let db = &ctx.download_ctx.db;

        let manifest = manifest_for_test()?;
        let new_manifest = manifest_for_test2()?;

        initialize_video_entries(db, &manifest).await.or_fail()?;

        for video in manifest.sections.iter().flat_map(|s| s.content.iter()) {
            // Create a partial video file that should be deleted
            let p = ctx
                .download_ctx
                .config
                .content_path
                .join(format!("{}.mp4", video.id));

            tokio::fs::write(p, b"Dummy content").await.or_fail()?;
        }

        remove_old_video_content(&ctx.download_ctx.config.content_path, db, &new_manifest)
            .await
            .or_fail()?;

        for video in manifest.sections.iter().flat_map(|s| s.content.iter()) {
            let db_video = db.find_video(video.id).await;

            let in_new_manifest = new_manifest
                .sections
                .iter()
                .flat_map(|s| s.content.iter())
                .any(|v| v.id == video.id);

            let p = ctx
                .download_ctx
                .config
                .content_path
                .join(format!("{}.mp4", video.id));

            if in_new_manifest {
                expect_that!(
                    db_video,
                    ok(eq(&crate::db::Video {
                        id: video.id,
                        name: video.name.clone(),
                        file_size: video.file_size,
                        download_status: crate::db::DownloadStatus::Pending,
                        view_count: 0,
                    }))
                );
                let content = tokio::fs::read_to_string(p).await.or_fail()?;
                expect_that!(content, eq("Dummy content"));
            } else {
                expect_that!(
                    db_video,
                    err(matches_pattern!(crate::db::Error::Diesel(
                        matches_pattern!(diesel::result::Error::NotFound)
                    )))
                );
                expect_false!(p.exists());
            }
        }

        Ok(())
    }

    #[tokio::test]
    #[googletest::test]
    async fn test_download_job_task_recoverable_io_failure() -> googletest::Result<()> {
        let ctx = create_context().await;
        let id = uuid::Uuid::from_str("5eb9e089-79cf-478d-9121-9ca3e7bb1d4a").or_fail()?;

        initialize_video_entries(&ctx.download_ctx.db, &manifest_for_test().or_fail()?)
            .await
            .or_fail()?;

        let result = download_job_task(
            ctx.download_ctx.clone(),
            Job {
                backoff_time: ctx.download_ctx.config.retry_params.initial_backoff,
                video: Video {
                    name: "Quadratic equations".to_string(),
                    id,
                    uri: "s3://bucket/quadratic-equations.mp4".parse().or_fail()?,
                    sha256: "8f9e3a4ae7d86c4abdf731a947fc90b607b82a0362da0b312e3b644defedb81f"
                        .try_into()
                        .or_fail()?,
                    file_size: 123457,
                },
            },
        )
        .await;

        assert_that!(
            &result,
            err(all!(
                matches_pattern!(DownloadJobError::ShouldRetry(_)),
                property!(
                    DownloadJobError.to_job(),
                    matches_pattern!(Job {
                        video: matches_pattern!(Video { id: &id, .. }),
                        backoff_time: &ctx.download_ctx.config.retry_params.initial_backoff,
                    })
                )
            ))
        );

        // Check that file is available in the database
        let db_video = ctx.download_ctx.db.find_video(id).await.or_fail()?;
        expect_that!(
            db_video,
            matches_pattern!(crate::db::Video {
                id: &id,
                download_status: matches_pattern!(crate::db::DownloadStatus::Failed(eq(
                    "Error fetching file with id: 5eb9e089-79cf-478d-9121-9ca3e7bb1d4a, name: Quadratic equations. path: s3://bucket/quadratic-equations.mp4. Error: I/O error reading from backend: ."
                ))),
                ..
            })
        );

        Ok(())
    }

    #[tokio::test]
    #[googletest::test]
    async fn test_download_job_task_successful() -> googletest::Result<()> {
        let ctx = create_context().await;
        let name = "Quadratic equations".to_string();
        let id = uuid::Uuid::from_str("5eb9e089-79cf-478d-9121-9ca3e7bb1d4a").or_fail()?;
        let uri: Uri = "s3://bucket/quadratic-equations.mp4".parse().or_fail()?;

        ctx.dummy_backend
            .add_file(BackendFile {
                uri: uri.clone(),
                content: vec![1, 2, 3, 4],
            })
            .await;

        initialize_video_entries(&ctx.download_ctx.db, &manifest_for_test().or_fail()?)
            .await
            .or_fail()?;

        let result = download_job_task(
            ctx.download_ctx.clone(),
            Job {
                backoff_time: ctx.download_ctx.config.retry_params.initial_backoff,
                video: Video {
                    name: name.clone(),
                    id,
                    uri,
                    sha256: "9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a"
                        .try_into()
                        .or_fail()?,
                    file_size: 4,
                },
            },
        )
        .await;

        assert_that!(result, ok(anything()));

        // Check that file is available in the database
        let video_fs_path = ctx
            .download_ctx
            .config
            .content_path
            .join(format!("{id}.mp4"));
        let db_video = ctx.download_ctx.db.find_video(id).await.or_fail()?;
        expect_that!(
            db_video,
            matches_pattern!(crate::db::Video {
                id: &id,
                download_status: &crate::db::DownloadStatus::Downloaded(video_fs_path.clone()),
                ..
            })
        );

        // Check that file is available in the filesystem
        let data = tokio::fs::read(video_fs_path).await.or_fail()?;
        assert_that!(data, eq(&vec![1, 2, 3, 4]));

        Ok(())
    }

    #[tokio::test]
    #[googletest::test]
    async fn test_download_job_task_invalid_checksum() -> googletest::Result<()> {
        let ctx = create_context().await;
        let name = "Quadratic equations".to_string();
        let id = uuid::Uuid::from_str("5eb9e089-79cf-478d-9121-9ca3e7bb1d4a").or_fail()?;
        let uri: Uri = "s3://bucket/quadratic-equations.mp4".parse().or_fail()?;

        ctx.dummy_backend
            .add_file(BackendFile {
                uri: uri.clone(),
                content: vec![1, 2, 3, 5],
            })
            .await;

        initialize_video_entries(&ctx.download_ctx.db, &manifest_for_test().or_fail()?)
            .await
            .or_fail()?;

        let result = download_job_task(
            ctx.download_ctx.clone(),
            Job {
                backoff_time: ctx.download_ctx.config.retry_params.initial_backoff,
                video: Video {
                    name: name.clone(),
                    id,
                    uri,
                    sha256: "9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a"
                        .try_into()
                        .or_fail()?,
                    file_size: 4,
                },
            },
        )
        .await;

        assert_that!(
            &result,
            err(all!(
                matches_pattern!(DownloadJobError::ShouldRetry(_)),
                property!(
                    DownloadJobError.to_job(),
                    matches_pattern!(Job {
                        video: matches_pattern!(Video { id: &id, .. }),
                        backoff_time: &ctx.download_ctx.config.retry_params.initial_backoff,
                    })
                )
            ))
        );

        // Check that file is available in the database
        let db_video = ctx.download_ctx.db.find_video(id).await.or_fail()?;
        expect_that!(
            db_video,
            matches_pattern!(crate::db::Video {
                id: &id,
                download_status: matches_pattern!(crate::db::DownloadStatus::Failed(eq(
                    "Got hash: 1571902abec0a45661de965dbe90cb0177b98c49fc58a5aabfa1edb6c678d972. Expected: 9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a"
                ))),
                ..
            })
        );

        Ok(())
    }
}
