//! Handlers for the content and versioning API endpoints.
//!
//! This module provides endpoints for:
//! - Fetching the current build information.
//! - Listing content metadata.
//! - Fetching specific content metadata by ID.
//! - Streaming content (videos).
//! - Incrementing view counts.
//! - Fetching the latest manifest.
//! - Fetching the system log file.

use std::str::FromStr;

use actix_web::{
    HttpRequest, HttpResponse, Responder, get, post,
    web::{self, Bytes, BytesMut},
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tracing::instrument::Instrument;

use leap_api::api::content::meta::get::{GroupedSection, LocalVideoMeta, Progress, VideoStatus};

use crate::{api::ApiData, downloader::UserCommand};

impl From<crate::db::DownloadStatus> for VideoStatus {
    fn from(value: crate::db::DownloadStatus) -> Self {
        match value {
            crate::db::DownloadStatus::Pending => VideoStatus::Pending,
            crate::db::DownloadStatus::InProgress((completed, total)) => {
                VideoStatus::Downloading(Progress(completed as f64 / total as f64))
            }
            crate::db::DownloadStatus::Downloaded(_) => VideoStatus::Downloaded,
            crate::db::DownloadStatus::Failed(msg) => VideoStatus::Failed(msg),
        }
    }
}

impl From<crate::db::Video> for LocalVideoMeta {
    fn from(value: crate::db::Video) -> Self {
        LocalVideoMeta {
            id: value.id.to_string(),
            name: value.name,
            size: value.file_size as usize,
            status: value.download_status.into(),
            view_count: value.view_count,
        }
    }
}

impl From<crate::build_info::BuildInfo> for leap_api::api::version::get::BuildInfo {
    fn from(value: crate::build_info::BuildInfo) -> Self {
        Self {
            name: value.name.to_string(),
            version: value.version.to_string(),
            git_hash: value.git_hash,
            authors: value.authors,
            homepage: value.homepage.to_string(),
            license: value.license.to_string(),
            repository: value.repository.to_string(),
            profile: value.profile.to_string(),
            rustc_version: value.rustc_version.to_string(),
            features: value.features.to_string(),
        }
    }
}

#[tracing::instrument(
    fields(
        request_id = %uuid::Uuid::new_v4(),
    )
)]
/// Returns the current build information.
///
/// This endpoint provides information about the current version, git hash, and build profile.
#[get("/version")]
async fn get_version() -> impl Responder {
    let info = crate::build_info::get();
    let info: leap_api::api::version::get::Response = info.into();
    HttpResponse::Ok().json(info)
}

#[tracing::instrument(
    skip(api_data)
    fields(
        request_id = %uuid::Uuid::new_v4(),
    )
)]
/// Lists all content metadata currently available in the server.
///
/// This endpoint returns a list of sections, each containing a list of video metadata.
#[get("/content/meta")]
async fn list_content_metadata(api_data: web::Data<ApiData>) -> impl Responder {
    use leap_api::api::content::meta::get::Response;

    let sections = match api_data
        .db
        .current_manifest_sections()
        .instrument(tracing::info_span!(
            "Querying manifest information from database"
        ))
        .await
    {
        Ok(sections) => sections,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .body(format!("Unexpected error querying content list: {e:?}"));
        }
    };

    let _span =
        tracing::info_span!("Collecting manifest information as /content/meta response").entered();

    let videos = sections
        .into_iter()
        .map(|(name, content)| {
            let content = content.into_iter().map(|v| v.into()).collect();
            GroupedSection { name, content }
        })
        .collect();

    HttpResponse::Ok().json(Response { videos })
}

#[tracing::instrument(
    skip(api_data)
    fields(
        request_id = %uuid::Uuid::new_v4(),
        %id
    )
)]
/// Returns metadata for a specific video by its ID.
///
/// Returns `400 Bad Request` if the ID is invalid.
/// Returns `200 OK` with `null` if the video is not found.
/// Returns `500 Internal Server Error` if a database error occurs.
#[get("/content/meta/{id}")]
async fn content_metadata_for_id(
    api_data: web::Data<ApiData>,
    id: web::Path<String>,
) -> impl Responder {
    use leap_api::api::content::meta::id::get::Response;
    let Ok(id) = id.into_inner().try_into() else {
        return HttpResponse::BadRequest().body("Invalid video ID");
    };

    let meta = match api_data
        .db
        .find_video(id)
        .instrument(tracing::info_span!("Obtaining video information from DB"))
        .await
    {
        Ok(meta) => Some(meta.into()),
        Err(crate::db::Error::Diesel(diesel::result::Error::NotFound)) => None,
        Err(err) => {
            tracing::error!("The database failed with code: {err}");
            return HttpResponse::InternalServerError()
                .body(format!("Error querying the video from database: {err}"));
        }
    };

    HttpResponse::Ok().json(Response { meta })
}

#[tracing::instrument(
    skip(api_data)
    fields(
        request_id = %uuid::Uuid::new_v4(),
        %id
    )
)]
/// Streams the content of a specific video.
///
/// This endpoint supports HTTP Range requests for partial content streaming,
/// which is essential for seeking within video files.
///
/// Returns `400 Bad Request` if the ID is invalid.
/// Returns `404 Not Found` if the video is not available.
/// Returns `500 Internal Server Error` if there is an issue reading the file or the database.
#[get("/content/{id}")]
async fn get_content(
    api_data: web::Data<ApiData>,
    id: web::Path<String>,
    request: HttpRequest,
) -> impl Responder {
    let Ok(id) = id.into_inner().try_into() else {
        let msg = "Invalid video ID";
        tracing::error!(msg);
        return HttpResponse::BadRequest().body(msg);
    };
    let Ok(crate::db::Video {
        download_status: crate::db::DownloadStatus::Downloaded(filepath),
        ..
    }) = api_data.db.find_video(id).await
    else {
        let msg = "Requested video ID is not available";
        tracing::error!(msg);
        return HttpResponse::NotFound().body(msg);
    };

    let mut file = match tokio::fs::File::open(&filepath).await {
        Ok(file) => file,
        Err(e) if e.kind() == tokio::io::ErrorKind::NotFound => {
            let msg = "Requested video is not on disk";
            tracing::error!(msg);
            return HttpResponse::InternalServerError().body(msg);
        }
        Err(e) => {
            let msg = format!("Unexpected error opening file: {e:?}");
            tracing::error!(msg);
            return HttpResponse::InternalServerError().body(msg);
        }
    };

    let meta = match tokio::fs::metadata(&filepath).await {
        Ok(meta) => meta,
        Err(e) => {
            let msg = format!("Unexpected error getting metadata for file: {e:?}");
            tracing::error!(msg);
            return HttpResponse::InternalServerError().body(msg);
        }
    };

    let total_length = meta.len();

    let mut req_length = meta.len();

    let range = request
        .headers()
        .iter()
        .find(|(name, _)| *name == "Range")
        .and_then(|(_, v)| {
            v.to_str()
                .inspect_err(|e| tracing::error!("Range header contains non-str value {e}"))
                .ok()
                .and_then(|v| {
                    actix_web::http::header::Range::from_str(v)
                        .inspect_err(|e| tracing::error!("Invalid range request: {e}"))
                        .ok()
                })
        })
        .and_then(|v| match v {
            actix_web::http::header::Range::Bytes(ranges) => {
                if ranges.len() != 1 {
                    tracing::error!(
                        "Only one byte range is currently supported, but got {ranges:?}"
                    );
                    None
                } else {
                    ranges[0]
                        .to_satisfiable_range(total_length)
                        .inspect(|(b, e)| tracing::debug!("Range request: {b}-{e}"))
                }
            }
            actix_web::http::header::Range::Unregistered(b, e) => {
                tracing::error!("Unsupported unregistered range request: {b}-{e}");
                None
            }
        });

    if let Some((begin, end)) = &range {
        match file.seek(std::io::SeekFrom::Start(*begin)).await {
            Ok(v) => v,
            Err(e) => {
                let msg = format!("Unexpected seeking file to fulfill range request: {e:?}");
                tracing::error!(msg);
                return HttpResponse::InternalServerError().body(msg);
            }
        };
        req_length = end - begin + 1;
    }

    const RESPONSE_CHUNK_SIZE: u64 = 4096;
    let s = async_stream::stream! {
        while req_length > 0 {
            // Note we are using a new bytes instance each time on purpose. We could have used
            // `split()` to get the current bytes out and reuse the instance. However, that makes
            // the bytes turn into a shared instance, which only releases the bytes once all
            // references to each of the chunks are dropped.
            //
            // This would not meet the intent of this code, which is to reduce the memory footprint
            // of this HTTP method, as some files might be hundreds of megabytes or even gigabytes
            // in size, and we only have 1 GiB of RAM for the entire platform.
            let mut bytes = BytesMut::with_capacity(RESPONSE_CHUNK_SIZE as usize);
            let current_chunk = req_length.min(RESPONSE_CHUNK_SIZE);
            bytes.resize(current_chunk as usize, 0);
            let Ok(n) = file.read_exact(&mut bytes).await else {
                let msg = "Unable to read data from file";
                tracing::error!(msg);
                yield Err::<Bytes, anyhow::Error>(anyhow::anyhow!(msg));
                return;
            };
            if n == 0 {
                return;
            }
            req_length -= current_chunk;
            yield Ok::<Bytes, anyhow::Error>(bytes.freeze());
        }
    };

    if let Some((begin, end)) = range {
        HttpResponse::PartialContent()
            .content_type("video/mp4")
            .append_header((
                "Content-Range",
                format!("bytes {begin}-{end}/{total_length}"),
            ))
            .streaming(Box::pin(s))
    } else {
        HttpResponse::Ok()
            .content_type("video/mp4")
            .streaming(Box::pin(s))
    }
}

#[tracing::instrument(
    skip(api_data)
    fields(
        request_id = %uuid::Uuid::new_v4(),
        %id
    )
)]
/// Increments the view count for a specific video.
///
/// Returns `400 Bad Request` if the ID is invalid.
/// Returns `404 Not Found` if the video is not found.
#[post("/content/{id}/view")]
async fn increment_view_cnt(api_data: web::Data<ApiData>, id: web::Path<String>) -> impl Responder {
    let Ok(id) = id.into_inner().try_into() else {
        return HttpResponse::BadRequest().body("Invalid video ID");
    };
    let Ok(crate::db::Video {
        download_status: crate::db::DownloadStatus::Downloaded(_),
        ..
    }) = api_data.db.increment_view_count(id).await
    else {
        let msg = "Requested video ID is not available";
        tracing::error!(msg);
        return HttpResponse::NotFound().body(msg);
    };
    HttpResponse::Ok().finish()
}

#[tracing::instrument(
    skip(api_data)
    fields(
        request_id = %uuid::Uuid::new_v4(),
    )
)]
/// Returns the current manifest in JSON format.
#[get("/manifest/latest")]
async fn get_manifest(api_data: web::Data<ApiData>) -> impl Responder {
    let manifest = api_data.db.current_manifest().await;

    let manifest_file = manifest
        .as_ref()
        .and_then(|m| serde_json::to_string(m).ok())
        .unwrap_or_else(|| "".to_string());

    HttpResponse::Ok()
        .content_type("application/json")
        .body(manifest_file)
}

#[tracing::instrument(
    skip(api_data)
    fields(
        request_id = %uuid::Uuid::new_v4(),
    )
)]
/// Triggers a new manifest fetch.
///
/// This tells the downloader service to fetch the latest manifest from the remote source.
#[post("/manifest/fetch")]
async fn fetch_manifest(api_data: web::Data<ApiData>) -> impl Responder {
    match api_data.cmd_sender.send(UserCommand::FetchManifest) {
        Ok(()) => HttpResponse::Ok().finish(),
        Err(e) => {
            let msg = format!("Unable to handle request: {e}");
            tracing::error!(msg);
            HttpResponse::InternalServerError().body(msg)
        }
    }
}

#[tracing::instrument(
    skip(api_data)
    fields(
        request_id = %uuid::Uuid::new_v4(),
    )
)]
/// Returns the content of the system log file.
#[get("/logfile")]
async fn log_file(api_data: web::Data<ApiData>) -> impl Responder {
    let log = match tokio::fs::read_to_string(api_data.config.db_config.logfile()).await {
        Ok(log) => log,
        Err(e) => {
            let msg = format!("Unexpected error opening file: {e:?}");
            tracing::error!(msg);
            return HttpResponse::InternalServerError().body(msg);
        }
    };
    HttpResponse::Ok().body(log)
}
