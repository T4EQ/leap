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
pub mod utils;

mod api;
mod downloader;
mod manifest;
mod provision;
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
