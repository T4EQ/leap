//! API routing and shared data structures for the `leap-server`.
//!
//! This module defines the `ApiData` and `ProvisionApiData` structures which are
//! shared across HTTP handlers, and provides functions to register handlers for
//! both the main API and the provisioning API.

use std::sync::Arc;

use crate::provision::DynProvision;
use crate::{cfg::LeapConfig, db::Database, downloader::UserCommand};

use actix_web::web;
use tokio::sync::mpsc::UnboundedSender;

mod provision;
mod user;

/// Shared resources used in HTTP handlers.
pub struct ApiData {
    /// The LEAP configuration.
    config: LeapConfig,
    /// A handle to the database.
    db: Arc<Database>,
    /// A channel to send commands to the downloader service.
    cmd_sender: UnboundedSender<UserCommand>,
}

impl ApiData {
    pub fn new(
        config: LeapConfig,
        db: Arc<Database>,
        cmd_sender: UnboundedSender<UserCommand>,
    ) -> Self {
        Self {
            config,
            db,
            cmd_sender,
        }
    }
}

/// Shared resources used in provisioning HTTP handlers.
#[derive(Debug)]
pub struct ProvisionApiData {
    /// The provisioning engine.
    provision: DynProvision,
}

impl ProvisionApiData {
    pub async fn new() -> anyhow::Result<Self> {
        Ok(Self {
            provision: DynProvision::new().await?,
        })
    }
}

fn common_api_handlers() -> actix_web::Scope {
    web::scope("api").service(user::get_version)
}

/// Registers the main API handlers.
pub fn register_handlers(app: &mut web::ServiceConfig) {
    app.service(
        common_api_handlers()
            .service(user::list_content_metadata)
            .service(user::content_metadata_for_id)
            .service(user::get_content)
            .service(user::increment_view_cnt)
            .service(user::fetch_manifest)
            .service(user::get_manifest)
            .service(user::log_file),
    );
}

/// Registers the provisioning API handlers.
pub fn register_provisioning_handlers(app: &mut web::ServiceConfig) {
    app.service(common_api_handlers());
    app.service(
        web::scope("provision")
            .service(provision::set_network_config)
            .service(provision::get_storage_devs)
            .service(provision::format_storage)
            .service(provision::set_configuration)
            .service(provision::complete_provisioning)
            .service(provision::status),
    );
}
