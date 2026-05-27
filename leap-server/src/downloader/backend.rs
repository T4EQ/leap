//! Backend implementations for resource downloading.
//!
//! This module defines the [`Backend`] trait, which provides a common interface for
//! fetching resources and manifests from various sources (e.g., local file system,
//! remote HTTP servers).
//!
//! Implementations of [`Backend`] are used by the downloader to fetch content
//! listed in a [`crate::manifest::ManifestFile`].

use std::path::PathBuf;
use std::pin::Pin;

use crate::downloader::Error;

use async_stream::stream;
use tokio::io::AsyncReadExt;
use tokio_stream::Stream;

/// A result type representing a chunk of data fetched from a backend.
///
/// The error variant contains a [`crate::downloader::Error`].
pub type ChunkResult = Result<Vec<u8>, Error>;

/// A trait for providing access to resources and manifests.
///
/// Implementations of this trait are used by the downloader to fetch content
/// listed in a [`crate::manifest::ManifestFile`].
#[async_trait::async_trait]
pub trait Backend: Sync + Send {
    /// Fetches a resource from the given URI. Returns a stream of data chunks.
    ///
    /// The URI's path is used to identify the resource.
    ///
    /// Errors encountered during streaming (e.g., I/O errors) are yielded as
    /// `Err(crate::downloader::Error::IoError(..))` in the stream.
    fn fetch_resource<'a, 'b>(
        &'a self,
        uri: &'b http::Uri,
    ) -> Pin<Box<dyn Stream<Item = ChunkResult> + Send + 'a>>
    where
        'b: 'a;

    /// Obtains the current manifest from the upstream.
    ///
    /// Returns the manifest as a byte vector.
    ///
    /// Errors are returned as [`crate::downloader::Error`].
    async fn fetch_manifest(&self) -> Result<Vec<u8>, Error>;
}

const DEFAULT_CHUNK_SIZE: usize = 1024;

/// A backend implementation that fetches resources from the local file system.
///
/// This backend uses a base path as the root for all resource fetches.
pub struct FileBackend {
    base_path: PathBuf,
    chunk_size: usize,
}

impl FileBackend {
    /// Creates a new `FileBackend` with the given base path.
    ///
    /// The `base_path` is used as the root for all resource fetches.
    pub fn new(base_path: &std::path::Path) -> Self {
        let base_path = base_path.to_path_buf();
        Self {
            base_path,
            chunk_size: DEFAULT_CHUNK_SIZE,
        }
    }
}

#[async_trait::async_trait]
impl Backend for FileBackend {
    /// Fetches a resource from the local file system based on the URI path.
    ///
    /// The URI path is joined with the [`FileBackend`]'s `base_path` to locate the file.
    /// Leading slashes in the URI path are trimmed.
    ///
    /// Errors such as "file not found" are yielded as `Err(crate::downloader::Error::IoError(..))`
    /// in the stream.
    fn fetch_resource<'a, 'b>(
        &'a self,
        uri: &'b http::Uri,
    ) -> Pin<Box<dyn Stream<Item = ChunkResult> + Send + 'a>>
    where
        'b: 'a,
    {
        Box::pin(stream! {
            let relpath = uri.path().trim_start_matches(std::path::MAIN_SEPARATOR);
            let path = self.base_path.join(relpath);
            let mut file = tokio::fs::File::open(path).await?;

            loop {
                let mut chunk = vec![0; self.chunk_size];
                let n = file.read(&mut chunk[..]).await?;
                if n == 0 {
                    break;
                }
                chunk.resize(n, 0);
                yield Ok(chunk);
            }
        })
    }

    /// Fetches the manifest file from the base path.
    ///
    /// It looks for a file named `manifest.json` in the [`FileBackend`]'s `base_path`.
    ///
    /// Errors are returned as [`crate::downloader::Error`].
    async fn fetch_manifest(&self) -> Result<Vec<u8>, Error> {
        let manifest_path = self.base_path.join("manifest.json");
        Ok(tokio::fs::read(manifest_path).await?)
    }
}

#[cfg(test)]
mod test {
    use googletest::OrFail;
    use http::Uri;

    use super::*;

    use tokio_stream::StreamExt;

    #[googletest::test]
    #[tokio::test]
    async fn read_resource_using_file_backend() -> googletest::Result<()> {
        let temp_dir = tempfile::TempDir::new().or_fail()?;
        let resource_filepath = temp_dir.path().join("video.mp4");
        let v = vec![123; 8321];

        std::fs::write(&resource_filepath, &v[..]).or_fail()?;

        let backend = FileBackend::new(temp_dir.path());
        let uri = Uri::from_static("/video.mp4");
        let mut stream = backend.fetch_resource(&uri);

        let mut n_chunks = 0;
        let mut total_size = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.or_fail()?;
            total_size += chunk.len();
            n_chunks += 1;
        }

        assert_eq!(total_size, v.len());
        assert_eq!(n_chunks, v.len().div_ceil(DEFAULT_CHUNK_SIZE));

        Ok(())
    }
}
