//! Handles database updates by receiving watch notifications and triggering
//! a rebuild of the serialized state of the database.
//!
//! This serves as an optimization to reduce the work that is done per client
//! connection. By keeping a up-to-date single serialized state we can simply
//! forward the bytes to the client on every response without locking and
//! rebuilding the state for every client request.

use std::sync::Arc;

use tokio::sync::Notify;
use tokio::sync::watch;
use tracing::Instrument as _;

use leap_api::api::content::meta::get::{GroupedSection, Response as ContentMetaResponse};

use crate::db;

async fn content_metadata(db: &db::Database) -> db::Result<ContentMetaResponse> {
    let sections = db
        .current_manifest_sections()
        .instrument(tracing::info_span!(
            "Querying manifest information from database"
        ))
        .await?;

    let _span =
        tracing::info_span!("Collecting manifest information as content metadata").entered();
    let videos = sections
        .into_iter()
        .map(|(name, content)| {
            let content = content.into_iter().map(|v| v.into()).collect();
            GroupedSection { name, content }
        })
        .collect();

    Ok(ContentMetaResponse { videos })
}

pub struct StateEventHandler {
    task_cancel: Arc<Notify>,
    serialized_state_tx: watch::Sender<Arc<ContentMetaResponse>>,
}

impl StateEventHandler {
    pub async fn start(
        state_changed_notifier: Arc<Notify>,
        db: Arc<db::Database>,
    ) -> db::Result<Self> {
        let response = content_metadata(&db).await?;
        let (serialized_state_tx, _) = watch::channel(Arc::new(response));
        let task_cancel = Arc::new(Notify::new());
        tokio::spawn({
            let task_cancel = Arc::clone(&task_cancel);
            let serialized_state_tx = serialized_state_tx.clone();
            async move {
                loop {
                    tokio::select! {
                        biased;
                        _ = task_cancel.notified() => {
                            return
                        }
                        _ = state_changed_notifier.notified() =>  {
                            let meta = match content_metadata(&db).await {
                                Ok(meta) => meta,
                                Err(db_error) => {
                                    tracing::error!("Unable to emit state update due to db error: {db_error}");
                                    continue;
                                }
                            };
                            // TODO: emit proper data
                            let _ = serialized_state_tx.send(Arc::new(meta));
                        }

                    }
                }
            }
        });

        Ok(Self {
            task_cancel,
            serialized_state_tx,
        })
    }

    /// Creates a new receiver for the content metadata response.
    pub fn subscribe(&self) -> watch::Receiver<Arc<ContentMetaResponse>> {
        self.serialized_state_tx.subscribe()
    }
}

impl Drop for StateEventHandler {
    fn drop(&mut self) {
        self.task_cancel.notify_one();
    }
}
