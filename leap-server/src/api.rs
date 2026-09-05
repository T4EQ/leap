//! API routing and shared data structures for the `leap-server`.
//!
//! This module defines the `ApiData` and `ProvisionApiData` structures which are
//! shared across HTTP handlers, and provides functions to register handlers for
//! both the main API and the provisioning API.

use actix_web::web;

#[cfg(feature = "provision")]
mod provision;
mod user;

pub use user::ApiData;

#[cfg(feature = "provision")]
pub use provision::ProvisionApiData;

/// Returns the current build information.
///
/// This endpoint provides information about the current version, git hash, and build profile.
#[actix_web::get("/health_check")]
async fn health_check() -> impl actix_web::Responder {
    actix_web::HttpResponse::Ok()
}

fn common_api_handlers() -> actix_web::Scope {
    web::scope("api")
        .service(user::get_version)
        .service(health_check)
}

/// Registers the main API handlers.
pub fn register_handlers(app: &mut web::ServiceConfig) {
    app.service(
        common_api_handlers()
            .service(user::events)
            .service(user::list_content_metadata)
            .service(user::content_metadata_for_id)
            .service(user::get_content)
            .service(user::increment_view_cnt)
            .service(user::fetch_manifest)
            .service(user::get_manifest)
            .service(user::log_file),
    );
}

#[cfg(feature = "provision")]
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
