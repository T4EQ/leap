mod backend;
pub mod s3backend;
mod tasks;

use std::{path::PathBuf, sync::Arc};
use tokio::time::Duration;

use crate::{
    cfg::{DownloaderConfig, S3Config},
    db::Database,
};
use backend::FileBackend;
use s3backend::S3Backend;

use tokio::sync::mpsc::UnboundedReceiver;

/// Commands received from users
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UserCommand {
    /// User request to trigger an immediate manifest fetch
    FetchManifest,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I/O error reading from backend: {0}")]
    IoError(#[from] std::io::Error),
}

type DownloadJoinHandle = tokio::task::JoinHandle<anyhow::Result<()>>;

#[derive(Clone)]
struct DownloadContext {
    config: Arc<DownloaderConfig>,
    backend: Arc<dyn backend::Backend>,
    db: Arc<Database>,
}

#[tracing::instrument(name = "check_manifest_updates", skip(ctx, pending_task))]
async fn check_updates(
    ctx: DownloadContext,
    pending_task: &mut Option<DownloadJoinHandle>,
) -> anyhow::Result<()> {
    // Inspect new manifest file
    let Ok(manifest_data) = ctx.backend.fetch_manifest().await.inspect_err(|err| {
        tracing::error!("Error fetching manifest: {err}");
    }) else {
        return Ok(());
    };

    let Ok(new_manifest) = serde_json::from_slice(&manifest_data).inspect_err(|err| {
        tracing::error!("Received manifest with invalid format from the server: {err}");
    }) else {
        return Ok(());
    };

    let cur_manifest = ctx.db.current_manifest().await;
    let is_more_recent_manifest = cur_manifest
        .as_ref()
        .is_none_or(|v| *v != new_manifest && v.date.cmp(&new_manifest.date).is_lt());

    if !is_more_recent_manifest {
        // Nothing to do, the manifest has not changed
        tracing::info!(
            "Current Manifest dated on {} is up to date",
            cur_manifest.as_ref().unwrap().date
        );
        return Ok(());
    }
    drop(cur_manifest);

    tracing::info!("Found updated manifest dated on {}", new_manifest.date);

    // Note that we do not yet update the actual in-memory manifest, because we need to first make
    // sure that the db contains the corresponding entries
    ctx.db.save_manifest_to_disk(&manifest_data).await?;

    // Stop existing tasks, given we found an even more recent task
    if let Some(old_task) = pending_task.take() {
        if old_task.is_finished() {
            old_task.await??;
        } else {
            old_task.abort();
            match old_task.await {
                // This is a degenerate case in which the task is still able to finish even though
                // we cancelled it. It can happen due to race conditions.
                Ok(task_retval) => task_retval?,
                Err(e) if e.is_cancelled() => {
                    tracing::info!("Canceled previous download task in favor of a new task");
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
    }

    let download_manifest_task = tasks::download_manifest_task(ctx, new_manifest);
    pending_task.replace(tokio::task::spawn(download_manifest_task));

    Ok(())
}

#[tracing::instrument(name = "run_downloader", skip(config, db, cmd_receiver))]
pub async fn run_downloader(
    config: DownloaderConfig,
    s3_config: S3Config,
    db: Arc<Database>,
    mut cmd_receiver: UnboundedReceiver<UserCommand>,
) -> anyhow::Result<()> {
    let config = Arc::new(config);

    // The backend can be either a local file path or an S3 bucket. We allow local filepaths
    // for simple testing of the server.
    let backend: Arc<dyn backend::Backend> = match config.remote_server.scheme_str() {
        // If we don't have a scheme, we assume it is a file path
        None | Some("file") => {
            let path: PathBuf = config.remote_server.path().into();
            tracing::info!("Using file backend located at {path:?}");
            Arc::new(FileBackend::new(&path))
        }
        Some("s3") => {
            let bucket = config
                .remote_server
                .host()
                .ok_or_else(|| anyhow::anyhow!("S3 URI must specify a bucket name"))?;
            tracing::info!("Using S3 backend with bucket: {bucket}");

            Arc::new(S3Backend::new(bucket, &s3_config).await?)
        }
        Some(scheme) => {
            anyhow::bail!("Unknown remote server URI scheme: {scheme}");
        }
    };

    let download_context = DownloadContext {
        config,
        backend,
        db,
    };

    // Wait for time sync before starting anything
    tracing::info!("Waiting for time sync before starting downloader");
    let _ = crate::utils::wait_timesync(Duration::from_secs(30))
        .await
        .inspect(|_| tracing::info!("Time synchronized before downloader startup"))
        .inspect_err(|e| {
            tracing::error!("Time synchronization failed before downloader startup: {e:?}")
        });

    // We keep track of the last pending task so that we can cancel it if we discovered an
    // even-newer manifest
    let mut pending_task: Option<DownloadJoinHandle> = None;

    // Because the system might have restarted while downloading the current manifest, we
    // have to spawn a download task to verify that it is actually downloaded, or fetch whatever
    // is remaining.
    if let Some(cur_manifest) = download_context.db.current_manifest().await.clone() {
        tasks::mark_interrupted_downloads(&download_context.db, &cur_manifest).await?;
        let download_manifest_task =
            tasks::download_manifest_task(download_context.clone(), cur_manifest);
        pending_task.replace(tokio::task::spawn(download_manifest_task));
    } else {
        // Trigger initial fetch
        check_updates(download_context.clone(), &mut pending_task).await?;
    }

    loop {
        let mut wait = std::pin::pin!(tokio::time::sleep(download_context.config.update_interval));
        let cmd = tokio::select! {
            _ = &mut wait => { None }
            command = cmd_receiver.recv() => {
                command
            }
        };

        if let Some(UserCommand::FetchManifest) = cmd {
            tracing::info!("Handling user-requested fetch");
        }

        check_updates(download_context.clone(), &mut pending_task).await?;
    }
}
