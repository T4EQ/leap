//! Handlers for the provisioning API endpoints.
//!
//! This module provides endpoints for:
//! - Configuring network settings.
//! - Listing available storage devices.
//! - Formatting storage.
//! - Configuring the LEAP system.
//! - Completing the provisioning process.
//! - Checking the current provisioning status.

use std::path::PathBuf;

use actix_web::{HttpResponse, Responder, get, post, web};
use leap_api::provision::config::post::LeapConfig;
use leap_api::provision::network::post::NetworkConfig;
use leap_api::provision::storage::devices::get::BlockDevice;
use leap_api::types::DeviceType;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::api::ProvisionApiData;

impl From<crate::provision::BlockDeviceType> for DeviceType {
    fn from(value: crate::provision::BlockDeviceType) -> Self {
        match value {
            crate::provision::BlockDeviceType::Disk => Self::Disk,
            crate::provision::BlockDeviceType::Partition => Self::Partition,
        }
    }
}

impl From<crate::provision::BlockDevice> for BlockDevice {
    fn from(value: crate::provision::BlockDevice) -> Self {
        Self {
            name: value.name,
            size: value.size,
            device_type: value.ty.into(),
        }
    }
}

#[tracing::instrument(fields(request_id = %uuid::Uuid::new_v4()))]
/// Configures the network settings.
#[post("network")]
async fn set_network_config(
    provision_data: web::Data<Mutex<ProvisionApiData>>,
    web::Json(network_config): web::Json<NetworkConfig>,
) -> impl Responder {
    match provision_data.try_lock() {
        Ok(mut lock) => match lock.provision.configure_network(&network_config).await {
            Ok(()) => HttpResponse::Ok().body(""),
            Err(err) => HttpResponse::InternalServerError().body(format!("{err}")),
        },
        Err(_) => HttpResponse::BadRequest().body("Another provisioning operation is ongoing"),
    }
}

#[tracing::instrument(fields(request_id = %uuid::Uuid::new_v4()))]
/// Lists the available block devices.
#[get("storage/devices")]
async fn get_storage_devs(provision_data: web::Data<Mutex<ProvisionApiData>>) -> impl Responder {
    match provision_data.try_lock() {
        Ok(lock) => match lock.provision.list_blockdevs().await {
            Ok(blockdevs) => {
                let blockdevs: Vec<BlockDevice> = blockdevs.into_iter().map(|b| b.into()).collect();
                HttpResponse::Ok().json(blockdevs)
            }
            Err(err) => HttpResponse::InternalServerError().body(format!("{err}")),
        },
        Err(_) => HttpResponse::BadRequest().body("Another provisioning operation is ongoing"),
    }
}

#[derive(Deserialize)]
struct FormatStorageQuery {
    name: PathBuf,
}

#[tracing::instrument(fields(request_id = %uuid::Uuid::new_v4()))]
/// Formats a specific storage device.
#[post("storage/format")]
async fn format_storage(
    provision_data: web::Data<Mutex<ProvisionApiData>>,
    web::Query(FormatStorageQuery { name }): web::Query<FormatStorageQuery>,
) -> impl Responder {
    match provision_data.try_lock() {
        Ok(mut lock) => match lock.provision.configure_storage(&name).await {
            Ok(()) => HttpResponse::Ok().body(""),
            Err(err) => HttpResponse::InternalServerError().body(format!("{err}")),
        },
        Err(_) => HttpResponse::BadRequest().body("Another provisioning operation is ongoing"),
    }
}

#[tracing::instrument(fields(request_id = %uuid::Uuid::new_v4()))]
/// Configures the LEAP system.
#[post("config")]
async fn set_configuration(
    provision_data: web::Data<Mutex<ProvisionApiData>>,
    web::Json(config): web::Json<LeapConfig>,
) -> impl Responder {
    match provision_data.try_lock() {
        Ok(mut lock) => match lock.provision.configure_leap(&config).await {
            Ok(()) => HttpResponse::Ok().body(""),
            Err(err) => HttpResponse::InternalServerError().body(format!("{err}")),
        },
        Err(_) => HttpResponse::BadRequest().body("Another provisioning operation is ongoing"),
    }
}

#[tracing::instrument(fields(request_id = %uuid::Uuid::new_v4()))]
/// Completes the provisioning process.
#[post("complete")]
async fn complete_provisioning(
    provision_data: web::Data<Mutex<ProvisionApiData>>,
) -> impl Responder {
    match provision_data.try_lock() {
        Ok(mut lock) => match lock.provision.finish().await {
            Ok(()) => HttpResponse::Ok().body(""),
            Err(err) => HttpResponse::InternalServerError().body(format!("{err}")),
        },
        Err(_) => HttpResponse::BadRequest().body("Another provisioning operation is ongoing"),
    }
}

#[tracing::instrument(fields(request_id = %uuid::Uuid::new_v4()))]
/// Returns the current provisioning status, rebooting the system. This request is not expected to
/// return in the happy path.
#[get("status")]
async fn status(provision_data: web::Data<Mutex<ProvisionApiData>>) -> impl Responder {
    match provision_data.try_lock() {
        Ok(lock) => match lock.provision.status() {
            Ok(status) => HttpResponse::Ok().json(status),
            Err(err) => HttpResponse::InternalServerError().body(format!("{err}")),
        },
        Err(_) => HttpResponse::BadRequest().body("Another provisioning operation is ongoing"),
    }
}
