//! Database and Manifest management.
//!
//! This module provides an abstraction over:
//! - An SQLite database that handles the video status information.
//! - A manifest file saved directly in filesystem storage. This was simpler
//!   than coercing the manifest data into the database, which is complex
//!   due to the amount of tables that it would require (due to normalization).
//!
//! The `Database` struct manages both the SQLite connection pool and an in-memory
//! cache of the manifest file for fast access.

mod models;
mod schema;

use std::{path::Path, sync::Arc};

use crate::{cfg::DbConfig, manifest::ManifestFile};
pub use models::{DownloadStatus, Video};

use deadpool_diesel::{Manager, Pool};
use diesel::{connection::SimpleConnection, prelude::*};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use tokio::sync::RwLock;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Errors that can occur during database and manifest management.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// An error occurred while interacting with the connection pool.
    #[error("Pool error: {0:?}")]
    Pool(#[from] deadpool_diesel::PoolError),
    /// An error occurred while building the SQLite connection.
    #[error("Build error: {0:?}")]
    Build(#[from] deadpool_diesel::sqlite::BuildError),
    /// A Diesel error occurred.
    #[error("Diesel error: {0:?}")]
    Diesel(#[from] diesel::result::Error),
    /// An error occurred during database migration.
    #[error("Migration error")]
    Migration,
    /// The provided download status is invalid.
    #[error("Invalid download status: {0:?}")]
    InvalidDownloadStatus(i64),
    /// The provided UUID is invalid.
    #[error("Invalid uuid: {0:?}")]
    InvalidUUID(#[from] uuid::Error),
    /// An error occurred while saving the manifest file to disk.
    #[error("Error saving manifest: {0:?}")]
    ManifestSaveFailed(std::io::Error),
    /// A video is present in the manifest but not in the database.
    #[error("A video is not present in the DB but it is present in the manifest: {0}")]
    MissingVideoInDb(uuid::Uuid),
    /// A video being deleted is still present in the manifest.
    #[error("The video being deleted is still present in the manifest: {0}")]
    VideoIsStillInManifest(uuid::Uuid),
    /// An I/O error occurred.
    #[error("Filesystem error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = core::result::Result<T, Error>;

/// The main entry point for interacting with the database and manifest.
pub struct Database {
    config: DbConfig,
    pool: Pool<Manager<diesel::sqlite::SqliteConnection>>,
    // An in-memory copy of the manifest, for fast access to the data.
    current_manifest: Arc<RwLock<Option<ManifestFile>>>,
}

impl Database {
    /// Opens the database using the given configuration. Returns an error if the
    /// database could not be opened. Also loads the manifest file from storage.
    pub async fn open(config: DbConfig) -> Result<Self> {
        let db_path = config.db_path();
        if let Some(dir) = db_path.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        let url = db_path.to_string_lossy();
        let manager = Manager::new(url, deadpool_diesel::Runtime::Tokio1);
        let pool: Pool<Manager<_>> = Pool::builder(manager)
            .max_size(config.pool_size)
            .post_create(deadpool_diesel::sqlite::Hook::sync_fn(move |c, _m| {
                let mut c = c.lock().expect("poisoned mutex");
                c.batch_execute("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
                    .expect("Unable to configure journal mode on sqlite DB connection");
                c.batch_execute(&format!(
                    "PRAGMA busy_timeout = {};",
                    config.busy_timeout.as_millis()
                ))
                .expect("Unable to set busy timeout on DB connection");
                Ok(())
            }))
            .build()?;

        let manifest_path = config.manifest_path();
        let current_manifest: Arc<RwLock<Option<ManifestFile>>> = Arc::new(RwLock::new(
            tokio::fs::read(manifest_path)
                .await
                .ok()
                .and_then(|content| serde_json::from_slice(&content).ok()),
        ));

        Ok(Self {
            config,
            pool,
            current_manifest,
        })
    }

    /// The database may not yet exist on disk, or may have a format from previous versions of this
    /// software. Diesel manages database migrations for us and allows us to apply any pending
    /// migrations to the database so that we do not have to carry out these actions manually.
    ///
    /// This function performs any pending migrations, for either a non-existent database (being
    /// created now) or a database from a previous version of the software.
    pub async fn apply_pending_migrations(&self) -> Result<()> {
        let connection = self.pool.get().await?;
        connection
            .interact(move |conn| {
                conn.run_pending_migrations(MIGRATIONS)
                    .map_err(|_| Error::Migration)?;
                Ok(())
            })
            .await
            .expect("Unexpected panic of a background DB thread")
    }

    /// Saves the manifest file to disk, at the location indicated by the `runtime_path` in the
    /// `db_config` section of the database configuration.
    pub async fn save_manifest_to_disk(&self, manifest_data: &[u8]) -> Result<()> {
        // We follow a two-step approach here to prevent partial manifests being written to disk.
        // In the first step we write the manifest to a temporary path. This operation is assumed to be
        // non-atomic, and partial writes could happen.
        // In the second step, we rename the temporary manifest path to the actual manifest path. This
        // operation, while not being entirely atomic, is assumed to be as close as reasonably
        // possible.
        //
        // Even if the manifest is not saved appropriately (e.g.: if the process is killed while the
        // rename happens), the system should still recover afterwards by detecting the invalid
        // manifest.
        let temp_path = self.config.temp_manifest_path();
        tokio::fs::write(&temp_path, manifest_data)
            .await
            .map_err(Error::ManifestSaveFailed)?;

        let manifest_path = self.config.manifest_path();
        tokio::fs::rename(temp_path, manifest_path)
            .await
            .map_err(Error::ManifestSaveFailed)?;
        Ok(())
    }

    /// Publishes a manifest to make it available for the currently running software. For
    /// concurrency issues (to prevent a manifest which does not yet contain corresponding video
    /// entries in the database) this is decoupled from saving the manifest to disk, which can
    /// occur earlier (to ensure that the next boot uses the new manifest).
    pub async fn publish_manifest(&self, manifest_data: &ManifestFile) {
        self.current_manifest
            .write()
            .await
            .replace(manifest_data.clone());
    }

    /// Returns a the current manifest. The manifest will not be written until all read handles are
    /// dropped, so do not keep them for long periods of time.
    pub async fn current_manifest<'a, 's>(
        &'s self,
    ) -> tokio::sync::RwLockReadGuard<'a, Option<ManifestFile>>
    where
        's: 'a,
    {
        self.current_manifest.read().await
    }

    /// Returns the current manifest content divided by sections and ordered in the same way as the
    /// manifest (for both the sections and the videos within a section).
    pub async fn current_manifest_sections(&self) -> Result<Vec<(String, Vec<Video>)>> {
        let manifest_sections = self
            .current_manifest
            .read()
            .await
            .as_ref()
            .map(|manifest| manifest.sections.clone())
            .unwrap_or(vec![]);

        let ids: Vec<String> = manifest_sections
            .iter()
            .flat_map(|s| s.content.iter().map(|v| v.id.to_string()))
            .collect();

        let connection = self.pool.get().await?;
        let videos_from_db: Vec<Video> = connection
            .interact(move |conn| -> Result<Vec<Video>> {
                use schema::videos::dsl;

                Ok(dsl::videos
                    .filter(dsl::id.eq_any(ids))
                    .select(Video::as_select())
                    .get_results(conn)?)
            })
            .await
            .expect("Unexpected panic of a background DB thread")?;

        manifest_sections
            .into_iter()
            .map(|s| {
                s.content
                    .iter()
                    .map(|v| {
                        // Here we need to order the videos as in the manifes section.
                        // This is the reason why we can't just filter the videos matching relevant
                        // ids.
                        videos_from_db
                            .iter()
                            .find(|inner| inner.id == v.id)
                            .cloned()
                            .ok_or_else(|| Error::MissingVideoInDb(v.id))
                    })
                    .collect::<Result<Vec<Video>>>()
                    .map(|inner| (s.name, inner))
            })
            .collect()
    }

    /// Returns a list of all the videos in the database.
    pub async fn list_all_videos(&self) -> Result<Vec<Video>> {
        let connection = self.pool.get().await?;
        connection
            .interact(move |conn| {
                use schema::videos::dsl;

                let video: Vec<Video> = dsl::videos.select(Video::as_select()).get_results(conn)?;
                Ok(video)
            })
            .await
            .expect("Unexpected panic of a background DB thread")
    }

    /// Finds a video by UUID
    pub async fn find_video(&self, req_id: uuid::Uuid) -> Result<Video> {
        let req_id = req_id.to_string();

        let connection = self.pool.get().await?;
        connection
            .interact(move |conn| {
                use schema::videos::dsl;

                let video: Video = dsl::videos
                    .filter(dsl::id.eq(&req_id))
                    .select(Video::as_select())
                    .get_result(conn)?;
                Ok(video)
            })
            .await
            .expect("Unexpected panic of a background DB thread")
    }

    /// Deletes a video from the database. Ensure that this video is no longer referenced in the
    /// new manifest before deleting it, or this method will error.
    pub async fn delete_video(&self, req_id: uuid::Uuid) -> Result<()> {
        use schema::videos::dsl::*;

        let is_in_manifest = self
            .current_manifest
            .read()
            .await
            .as_ref()
            .is_some_and(|m| {
                m.sections
                    .iter()
                    .flat_map(|s| s.content.iter())
                    .any(|v| v.id == req_id)
            });
        if is_in_manifest {
            return Err(Error::VideoIsStillInManifest(req_id));
        }

        let req_id = req_id.to_string();
        let connection = self.pool.get().await?;
        connection
            .interact(move |c| {
                diesel::delete(videos.filter(id.eq(req_id))).execute(c)?;
                Ok(())
            })
            .await
            .expect("Unexpected panic of a background DB thread")
    }

    /// Inserts a new video into the database. Will return an error if the video is already
    /// present. Initializes the rest of the fields to default values.
    pub async fn insert_video(&self, id: uuid::Uuid, name: &str, file_size: u64) -> Result<()> {
        let id = id.to_string();
        let new_vid = models::NewVideo {
            id,
            name: name.to_string(),
            file_size: file_size as i64,
        };

        let connection = self.pool.get().await?;
        connection
            .interact(move |c| {
                diesel::insert_into(schema::videos::dsl::videos)
                    .values(new_vid)
                    .execute(c)?;
                Ok(())
            })
            .await
            .expect("Unexpected panic of a background DB thread")
    }

    /// Increments the viewed count for a given video.
    pub async fn increment_view_count(&self, req_id: uuid::Uuid) -> Result<Video> {
        let connection = self.pool.get().await?;
        connection
            .interact(move |c| -> Result<Video> {
                use schema::videos::dsl;
                Ok(diesel::update(dsl::videos.find(req_id.to_string()))
                    .set((dsl::view_count.eq(dsl::view_count + 1),))
                    .returning(Video::as_select())
                    .get_result(c)?)
            })
            .await
            .expect("Unexpected panic of a background DB thread")
    }

    /// Updates the download progress for a given video. `downloaded_size` should be
    /// smaller than the file size of the video.
    pub async fn update_download_progress(
        &self,
        req_id: uuid::Uuid,
        downloaded_size: u64,
    ) -> Result<()> {
        let connection = self.pool.get().await?;
        connection
            .interact(move |c| {
                use schema::videos::dsl;
                diesel::update(dsl::videos.find(req_id.to_string()))
                    .set((
                        dsl::download_status.eq(models::DOWNLOAD_STATUS_IN_PROGRESS),
                        dsl::downloaded_size.eq(downloaded_size as i64),
                        dsl::message.eq(""),
                    ))
                    .execute(c)?;
                Ok(())
            })
            .await
            .expect("Unexpected panic of a background DB thread")
    }

    /// Marks the given video as failed with the given error message.
    pub async fn set_download_failed(&self, req_id: uuid::Uuid, message: &str) -> Result<()> {
        let message = message.to_string(); // Need a copy since interact runs on a separate thread
        // and requires 'static.

        let connection = self.pool.get().await?;
        connection
            .interact(move |c| {
                use schema::videos::dsl;
                diesel::update(dsl::videos.find(req_id.to_string()))
                    .set((
                        dsl::download_status.eq(models::DOWNLOAD_STATUS_FAILED),
                        dsl::message.eq(message),
                    ))
                    .execute(c)?;
                Ok(())
            })
            .await
            .expect("Unexpected panic of a background DB thread")
    }

    /// Marks the given video as downloaded, at the given file path.
    pub async fn set_downloaded(&self, req_id: uuid::Uuid, file_path: &Path) -> Result<()> {
        let file_path = file_path.as_os_str().to_owned(); // Need a copy since interact runs on a separate thread
        // and requires 'static.

        let connection = self.pool.get().await?;
        connection
            .interact(move |c| {
                use schema::videos::dsl;
                diesel::update(dsl::videos.find(req_id.to_string()))
                    .set((
                        dsl::download_status.eq(models::DOWNLOAD_STATUS_DOWNLOADED),
                        dsl::downloaded_size.eq(dsl::file_size),
                        dsl::message.eq(""),
                        dsl::file_path.eq(file_path.as_encoded_bytes()),
                    ))
                    .execute(c)?;
                Ok(())
            })
            .await
            .expect("Unexpected panic of a background DB thread")
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use core::str::FromStr;
    use googletest::prelude::*;
    use std::{path::PathBuf, time::Duration};
    use tempfile::TempDir;

    fn create_dbconfig(runtime_path: &Path) -> DbConfig {
        DbConfig {
            busy_timeout: Duration::from_secs(2),
            runtime_path: runtime_path.into(),
            pool_size: 16,
        }
    }

    #[tokio::test]
    #[googletest::test]
    async fn test_open_db() -> googletest::Result<()> {
        let tempdir = TempDir::new().or_fail()?;
        let db_config = create_dbconfig(tempdir.path());
        let _db = Database::open(db_config).await.or_fail()?;
        Ok(())
    }

    #[tokio::test]
    #[googletest::test]
    async fn test_insert_and_get_video() -> googletest::Result<()> {
        let tempdir = TempDir::new().or_fail()?;
        let db_config = create_dbconfig(tempdir.path());
        let db = Database::open(db_config).await.or_fail()?;
        db.apply_pending_migrations().await.or_fail()?;

        let uuid = uuid::Uuid::from_str("bf978778-1c5d-44b3-b2c1-1cc253563799").or_fail()?;
        db.insert_video(uuid, "my video", 1234567).await.or_fail()?;

        let video = db.find_video(uuid).await.or_fail()?;
        expect_that!(
            video,
            eq(&Video {
                id: uuid,
                name: "my video".to_string(),
                file_size: 1234567,
                download_status: DownloadStatus::Pending,
                view_count: 0
            })
        );
        Ok(())
    }

    #[tokio::test]
    #[googletest::test]
    async fn test_increment_view_count() -> googletest::Result<()> {
        let tempdir = TempDir::new().or_fail()?;
        let db_config = create_dbconfig(tempdir.path());
        let db = Database::open(db_config).await.or_fail()?;
        db.apply_pending_migrations().await.or_fail()?;

        let uuid = uuid::Uuid::from_str("bf978778-1c5d-44b3-b2c1-1cc253563799").or_fail()?;
        db.insert_video(uuid, "my video", 1234567).await.or_fail()?;

        let incr_a = db.increment_view_count(uuid);
        let incr_b = db.increment_view_count(uuid);
        let incr_c = db.increment_view_count(uuid);

        let (res_a, res_b, res_c) = tokio::join!(incr_a, incr_b, incr_c);
        res_a.or_fail()?;
        res_b.or_fail()?;
        res_c.or_fail()?;

        let video = db.find_video(uuid).await.or_fail()?;
        expect_that!(
            video,
            eq(&Video {
                id: uuid,
                name: "my video".to_string(),
                file_size: 1234567,
                download_status: DownloadStatus::Pending,
                view_count: 3
            })
        );
        Ok(())
    }

    #[tokio::test]
    #[googletest::test]
    async fn test_download_progress() -> googletest::Result<()> {
        let tempdir = TempDir::new().or_fail()?;
        let db_config = create_dbconfig(tempdir.path());
        let db = Database::open(db_config).await.or_fail()?;
        db.apply_pending_migrations().await.or_fail()?;

        let uuid = uuid::Uuid::from_str("bf978778-1c5d-44b3-b2c1-1cc253563799").or_fail()?;
        db.insert_video(uuid, "my video", 1234567).await.or_fail()?;

        db.update_download_progress(uuid, 1234000).await?;

        let video = db.find_video(uuid).await.or_fail()?;
        expect_that!(
            video,
            eq(&Video {
                id: uuid,
                name: "my video".to_string(),
                file_size: 1234567,
                download_status: DownloadStatus::InProgress((1234000, 1234567)),
                view_count: 0
            })
        );

        db.update_download_progress(uuid, 1234400).await?;

        let video = db.find_video(uuid).await.or_fail()?;
        expect_that!(
            video,
            eq(&Video {
                id: uuid,
                name: "my video".to_string(),
                file_size: 1234567,
                download_status: DownloadStatus::InProgress((1234400, 1234567)),
                view_count: 0
            })
        );
        Ok(())
    }

    #[tokio::test]
    #[googletest::test]
    async fn test_downloaded() -> googletest::Result<()> {
        let tempdir = TempDir::new().or_fail()?;
        let db_config = create_dbconfig(tempdir.path());
        let db = Database::open(db_config).await.or_fail()?;
        db.apply_pending_migrations().await.or_fail()?;

        let uuid = uuid::Uuid::from_str("bf978778-1c5d-44b3-b2c1-1cc253563799").or_fail()?;
        db.insert_video(uuid, "my video", 1234567).await.or_fail()?;

        let pathbuf: PathBuf = "/path/to/the/file.mp4".into();
        db.set_downloaded(uuid, &pathbuf).await?;

        let video = db.find_video(uuid).await.or_fail()?;
        expect_that!(
            video,
            eq(&Video {
                id: uuid,
                name: "my video".to_string(),
                file_size: 1234567,
                download_status: DownloadStatus::Downloaded("/path/to/the/file.mp4".into()),
                view_count: 0
            })
        );

        Ok(())
    }

    #[tokio::test]
    #[googletest::test]
    async fn test_download_failed() -> googletest::Result<()> {
        let tempdir = TempDir::new().or_fail()?;
        let db_config = create_dbconfig(tempdir.path());
        let db = Database::open(db_config).await.or_fail()?;
        db.apply_pending_migrations().await.or_fail()?;

        let uuid = uuid::Uuid::from_str("bf978778-1c5d-44b3-b2c1-1cc253563799").or_fail()?;
        db.insert_video(uuid, "my video", 1234567).await.or_fail()?;

        db.set_download_failed(
            uuid,
            "Something failed, but I kid you not, I don't know what it is",
        )
        .await?;

        let video = db.find_video(uuid).await.or_fail()?;
        expect_that!(
            video,
            eq(&Video {
                id: uuid,
                name: "my video".to_string(),
                file_size: 1234567,
                download_status: DownloadStatus::Failed(
                    "Something failed, but I kid you not, I don't know what it is".to_string()
                ),
                view_count: 0
            })
        );

        Ok(())
    }

    fn manifest_for_test() -> googletest::Result<ManifestFile> {
        Ok(ManifestFile {
            name: "manifest".to_string(),
            date: chrono::NaiveDate::from_str("2025-10-10").or_fail()?,
            version: crate::manifest::Version {
                major: 2,
                minor: 0,
                revision: 0,
            },
            sections: vec![
                crate::manifest::Section {
                    name: "".to_string(),
                    content: vec![
                        crate::manifest::Video {
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
                        crate::manifest::Video {
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
                crate::manifest::Section {
                    name: "Integration".to_string(),
                    content: vec![
                        crate::manifest::Video {
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
                        crate::manifest::Video {
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

    #[tokio::test]
    #[googletest::test]
    async fn test_save_manifest_to_disk() -> googletest::Result<()> {
        let tempdir = TempDir::new().or_fail()?;
        let db_config = create_dbconfig(tempdir.path());
        let db = Database::open(db_config.clone()).await.or_fail()?;
        db.apply_pending_migrations().await.or_fail()?;

        let manifest = manifest_for_test()?;
        let manifest_data = serde_json::to_vec(&manifest).or_fail()?;
        db.save_manifest_to_disk(&manifest_data[..])
            .await
            .or_fail()?;

        drop(manifest_data);

        let manifest_data = tokio::fs::read(db_config.manifest_path()).await.or_fail()?;
        let stored_manifest: ManifestFile = serde_json::from_slice(&manifest_data[..]).or_fail()?;

        assert_that!(stored_manifest, eq(&manifest));

        Ok(())
    }

    #[tokio::test]
    #[googletest::test]
    async fn test_current_manifest_sections() -> googletest::Result<()> {
        let tempdir = TempDir::new().or_fail()?;
        let db_config = create_dbconfig(tempdir.path());
        let db = Database::open(db_config.clone()).await.or_fail()?;
        db.apply_pending_migrations().await.or_fail()?;

        let manifest = manifest_for_test()?;
        db.publish_manifest(&manifest).await;

        // Create db entries for each video
        for video in manifest.sections.iter().flat_map(|s| &s.content) {
            db.insert_video(video.id, &video.name, video.file_size)
                .await
                .or_fail()?;
        }

        let sections = db.current_manifest_sections().await.or_fail()?;

        assert_that!(sections.len(), eq(manifest.sections.len()));
        for ((name, content), manifest_section) in sections.iter().zip(manifest.sections) {
            expect_that!(name, eq(&manifest_section.name));
            expect_that!(content.len(), eq(manifest_section.content.len()));

            for (video, manifest_video) in content.iter().zip(manifest_section.content) {
                expect_that!(
                    video,
                    matches_pattern!(Video {
                        id: eq(&manifest_video.id),
                        name: eq(&manifest_video.name),
                        file_size: eq(&manifest_video.file_size),
                        download_status: eq(&DownloadStatus::Pending),
                        view_count: eq(&0),
                    })
                );
            }
        }

        Ok(())
    }
}
