use std::{net::TcpListener, path::PathBuf};

use clap::Parser;

#[derive(Parser, Debug)]
struct Args {
    /// Path to the LEAP configuration
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Fall back to provisioning if the LEAP configuration file could not be opened and parsed.
    #[arg(short = 'f', long = "fallback")]
    provision_fallback: bool,

    /// Run provisioning instead of the main application
    #[arg(short = 'p', long = "provision")]
    provision: bool,

    /// Address
    #[arg(long = "address", default_value = "0.0.0.0")]
    address: String,

    /// Port
    #[arg(long = "port", default_value = "80")]
    port: u16,

    /// Displays version information.
    #[arg(short, long)]
    version: bool,
}

fn default_config_path() -> PathBuf {
    leap_server::cfg::DEFAULT_CONFIG_PATH.into()
}

fn print_version_info() {
    let info = leap_server::build_info::get();
    println!("{}:", info.name);
    println!("\tVersion: {}", info.version);
    if let Some(git_hash) = &info.git_hash {
        println!("\tGit hash: {git_hash}");
    } else {
        println!("\tGit hash: Unknown");
    }
    println!("\tAuthors:");
    for author in info.authors {
        println!("\t\t{author}");
    }
    println!("\tHomepage: {}", info.homepage);
    println!("\tLicense: {}", info.license);
    println!("\tRepository: {}", info.repository);
    println!();
    println!("Build info:");
    println!("\tProfile: {}", info.profile);
    println!("\trustc version: {}", info.rustc_version);
    println!("\tFeatures: {}", info.features);
}

#[derive(thiserror::Error, Debug)]
enum AppError {
    #[error("The LEAP configuration could not be loaded: {0}")]
    InvalidConfiguration(anyhow::Error),
    #[error("LEAP failed during runtime: {0}")]
    RuntimeError(anyhow::Error),
}

async fn start_leap_server(args: &Args) -> Result<(), AppError> {
    let config =
        leap_server::cfg::get_config(args.config.as_ref().unwrap_or(&default_config_path()))
            .map_err(AppError::InvalidConfiguration)?;
    leap_server::init_logging(Some(&config.db_config.logfile()), config.debug).await;

    let listener = TcpListener::bind(format!("{}:{}", args.address, args.port))
        .map_err(|e| AppError::RuntimeError(e.into()))?;

    println!(
        "Started server at http://{}",
        listener
            .local_addr()
            .map_err(|e| AppError::RuntimeError(e.into()))?
    );
    leap_server::run_app(listener, config)
        .await
        .map_err(AppError::RuntimeError)?;
    Ok(())
}

#[cfg(feature = "provision")]
async fn start_leap_provisioning(args: &Args) -> anyhow::Result<()> {
    leap_server::init_logging(None, false).await;
    let listener = TcpListener::bind(format!("{}:{}", args.address, args.port))?;
    leap_server::run_provisioning(listener).await?;
    Ok(())
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.version {
        print_version_info();
        return Ok(());
    }

    std::cfg_select! {
        feature = "provision" => {
            if args.provision {
                start_leap_provisioning(&args).await?;
            } else {
                match start_leap_server(&args).await {
                    Err(AppError::InvalidConfiguration(error)) => {
                        if args.provision_fallback {
                            tracing::error!(
                                "Failed to start leap with the given configuration file. Falling back to provisioning: {error}"
                            );
                            start_leap_provisioning(&args).await?;
                        }
                        Err(AppError::InvalidConfiguration(error))
                    }
                    result => result,
                }?;
            }
        }
        _ => {
            start_leap_server(&args).await?;
        }
    }

    Ok(())
}
