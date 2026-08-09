//! Utility functions for the leap user API

use tracing::Instrument as _;

use leap_api::api::content::meta::get::{
    GroupedSection, ManifestMeta, Response as ContentMetaResponse,
};

use crate::db;

pub async fn content_metadata(db: &db::Database) -> db::Result<ContentMetaResponse> {
    let meta = db
        .current_manifest_meta()
        .instrument(tracing::info_span!(
            "Querying manifest information from database"
        ))
        .await?;

    let _span =
        tracing::info_span!("Collecting manifest information as content metadata").entered();

    let meta = meta.map(|meta| ManifestMeta {
        name: meta.name,
        date: meta.date,
        content: meta
            .sections
            .into_iter()
            .map(|(name, content)| {
                let content = content.into_iter().map(|v| v.into()).collect();
                GroupedSection { name, content }
            })
            .collect(),
    });

    Ok(ContentMetaResponse { meta })
}
