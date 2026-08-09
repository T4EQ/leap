//! Handles database updates by receiving watch notifications and triggering
//! a rebuild of the serialized state of the database.
//!
//! This serves as an optimization to reduce the work that is done per client
//! connection. By keeping a up-to-date single state we can simply forward the
//! response to the client without locking and rebuilding the state for every
//! client request.

use std::sync::Arc;

use tokio::sync::Notify;
use tokio::sync::watch;

use leap_api::api::content::meta::get::Response as ContentMetaResponse;

use super::utils::content_metadata;
use crate::db;

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
                                    // Self-notify to try again
                                    state_changed_notifier.notify_one();
                                    continue;
                                }
                            };
                            let _ = serialized_state_tx.send_replace(Arc::new(meta));

                            // Throttle the event updates to limit the notification rate to roughly
                            // 200 ms
                            const MAX_UPDATE_RATE: std::time::Duration = std::time::Duration::from_millis(200);
                            tokio::time::sleep(MAX_UPDATE_RATE).await;
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
