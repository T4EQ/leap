//! The `leap-server` crate provides the core server functionality for Leap.
//!
//! It is responsible for managing the main application server, which includes
//! an API, a database connection, and a background downloader task. Additionally,
//! it provides a provisioning server for device setup.
//!
//! ## Main Components
//!
//! - **Application Server**: The primary entry point for interacting with Leap,
//!   providing the main API via [`run_app`].
//! - **Provisioning Server**: A specialized server for provisioning devices via [`run_provisioning`].
//! - **Downloader**: A background task managed by the application server that handles
//!   asynchronous downloads.
//! - **Database**: Manages persistent storage via the [`db`] module.
//!
//! ## Usage Guide
//!
//! To start the main application, use [`run_app`] with a [`TcpListener`] and a [`LeapConfig`].
//!
//! ```rust,no_run
//! use leap_server::{run_app, cfg::LeapConfig};
//! use std::net::TcpListener;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let listener = TcpListener::bind("127.0.0.1:8080")?;
//!     let config = leap_server::cfg::get_config(&Path::new("config.toml"))?;
//!     run_app(listener, config).await?;
//!     Ok(())
//! }
//! ```
//!
//! To start the provisioning server, use [`run_provisioning`]:
//!
//! ```rust,no_run
//! use leap_server::run_provisioning;
//! use std::net::TcpListener;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let listener = TcpListener::bind("127.0.0.1:9000")?;
//!     run_provisioning(listener).await?;
//!     Ok(())
//! }
//! ```
//!
use actix_web::{App, HttpServer, web};
use anyhow::Context;
use tokio::sync::{Mutex, mpsc};
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use std::{io::stdout, net::TcpListener, path::Path, sync::Arc};

use crate::{api::ProvisionApiData, cfg::LeapConfig};

pub mod build_info;
pub mod cfg;
pub mod db;
pub mod downloader;
pub mod manifest;
pub mod provision;
pub mod utils;

mod api;
mod static_files;

pub async fn init_logging(logfile: Option<&Path>, debug: bool) {
    let layered = tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                let level = if debug { "trace" } else { "info" };
                tracing_subscriber::EnvFilter::new(level)
            }),
        )
        .with(JsonStorageLayer)
        .with(BunyanFormattingLayer::new("leap-server".into(), stdout));

    if let Some(logfile) = logfile {
        let logfile = logfile.to_owned();
        let open_logfile = {
            move || -> Box<dyn std::io::Write> {
                Box::new(
                    std::fs::File::options()
                        .create(true)
                        .append(true)
                        .open(&logfile)
                        .map_err(|e| format!("Unable to open logfile {logfile:?}: {e}"))
                        .unwrap(),
                )
            }
        };

        layered
            .with(BunyanFormattingLayer::new(
                "leap-server".into(),
                open_logfile,
            ))
            .init();
    } else {
        layered.init();
    }
}

pub async fn run_provisioning(listener: TcpListener) -> anyhow::Result<()> {
    let app_data = web::Data::new(Mutex::new(ProvisionApiData::new().await?));
    let server = HttpServer::new(move || {
        App::new()
            .app_data(app_data.clone())
            .wrap(tracing_actix_web::TracingLogger::default())
            .configure(api::register_provisioning_handlers)
            .configure(static_files::register_provisioning_files)
    })
    .listen(listener)?
    .run();

    Ok(server.await?)
}

pub async fn run_app(listener: TcpListener, config: LeapConfig) -> anyhow::Result<()> {
    let database = Arc::new(
        db::Database::open(config.db_config.clone())
            .await
            .context("While initializing database")?,
    );

    database.apply_pending_migrations().await?;

    let (user_command_sender, user_command_receiver) = mpsc::unbounded_channel();

    let downloader = downloader::run_downloader(
        config.downloader_config.clone(),
        config.s3_config.clone(),
        Arc::clone(&database),
        user_command_receiver,
    );

    let api_data = web::Data::new(api::ApiData::new(
        config.clone(),
        Arc::clone(&database),
        user_command_sender,
    ));

    let server = HttpServer::new(move || {
        App::new()
            .app_data(api_data.clone())
            .wrap(tracing_actix_web::TracingLogger::default())
            .configure(api::register_handlers)
            .configure(static_files::register_site_files)
    })
    .listen(listener)?
    .run();

    tokio::select! {
        downloader = downloader => {
            downloader?;
            panic!("Unexpected downloader task exit.");
        }
        server = server => {
            server?;
            // the server can exit due to SIGINT. Using join for these 2 futures would not
            // terminate the application because downloader would keep running indefinitely
        }
    };

    Ok(())
}
