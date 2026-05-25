//! Database models for the `leap-server`.
//!
//! This module defines the core data structures that map to the database tables,
//! including `Video` and `DownloadStatus`.

use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};

use diesel::{
    prelude::*,
    sql_types::{BigInt, Binary, Text},
};

use super::schema;

/// Represents the current download status of a video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadStatus {
    /// The download has not started yet.
    Pending,
    /// The download failed with the specified error message.
    Failed(String),
    /// The download is in progress, with the current amount of bytes downloaded and total bytes.
    InProgress((u64, u64)),
    /// The download is completed and the file is located at the specified path.
    Downloaded(PathBuf),
}

impl DownloadStatus {
    pub fn is_downloaded(&self) -> bool {
        matches!(self, DownloadStatus::Downloaded(_))
    }
}

impl Selectable<diesel::sqlite::Sqlite> for DownloadStatus {
    type SelectExpression = (
        schema::videos::dsl::file_size,
        schema::videos::dsl::downloaded_size,
        schema::videos::dsl::download_status,
        schema::videos::dsl::message,
        schema::videos::dsl::file_path,
    );

    fn construct_selection() -> Self::SelectExpression {
        (
            schema::videos::dsl::file_size,
            schema::videos::dsl::downloaded_size,
            schema::videos::dsl::download_status,
            schema::videos::dsl::message,
            schema::videos::dsl::file_path,
        )
    }
}

impl Queryable<(BigInt, BigInt, BigInt, Text, Binary), diesel::sqlite::Sqlite> for DownloadStatus {
    type Row = (i64, i64, i64, String, Vec<u8>);

    fn build(
        (file_size, downloaded_size, download_status, message, file_path): Self::Row,
    ) -> diesel::deserialize::Result<Self> {
        Ok(match download_status {
            DOWNLOAD_STATUS_NOT_STARTED => DownloadStatus::Pending,
            DOWNLOAD_STATUS_FAILED => DownloadStatus::Failed(message),
            DOWNLOAD_STATUS_IN_PROGRESS => {
                DownloadStatus::InProgress((downloaded_size as u64, file_size as u64))
            }
            DOWNLOAD_STATUS_DOWNLOADED => {
                DownloadStatus::Downloaded(OsString::from_vec(file_path).into())
            }
            v => {
                return Err(super::Error::InvalidDownloadStatus(v).into());
            }
        })
    }
}

pub const DOWNLOAD_STATUS_NOT_STARTED: i64 = 0;
pub const DOWNLOAD_STATUS_FAILED: i64 = 1;
pub const DOWNLOAD_STATUS_IN_PROGRESS: i64 = 2;
pub const DOWNLOAD_STATUS_DOWNLOADED: i64 = 3;

/// Represents a video entry in the database.
#[derive(Queryable, Debug, Clone, PartialEq, Eq)]
#[diesel(table_name = schema::videos)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Video {
    /// The unique identifier for the video.
    #[diesel(deserialize_as = String)]
    pub id: uuid::Uuid,

    /// The name of the video.
    pub name: String,

    /// The total file size of the video in bytes.
    #[diesel(deserialize_as = i64)]
    pub file_size: u64,

    /// The current download status of the video.
    pub download_status: DownloadStatus,

    /// The number of times the video has been viewed.
    #[diesel(deserialize_as = i64)]
    pub view_count: u64,
}

impl Selectable<diesel::sqlite::Sqlite> for Video {
    type SelectExpression = (
        schema::videos::dsl::id,
        schema::videos::dsl::name,
        schema::videos::dsl::file_size,
        <DownloadStatus as Selectable<diesel::sqlite::Sqlite>>::SelectExpression,
        schema::videos::dsl::view_count,
    );

    fn construct_selection() -> Self::SelectExpression {
        (
            schema::videos::dsl::id,
            schema::videos::dsl::name,
            schema::videos::dsl::file_size,
            <DownloadStatus as Selectable<diesel::sqlite::Sqlite>>::construct_selection(),
            schema::videos::dsl::view_count,
        )
    }
}

#[derive(Insertable)]
#[diesel(table_name = schema::videos)]
pub struct NewVideo {
    pub id: String,
    pub name: String,
    pub file_size: i64,
}
