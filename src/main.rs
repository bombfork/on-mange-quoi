mod i18n;
mod web;

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, bail};
use clap::Parser;

use crate::i18n::{MESSAGES, interpolate};

#[derive(Debug, Parser)]
#[command(version, about = MESSAGES.cli.about.as_str())]
struct Cli {
    #[arg(long, value_name = "DIR", help = MESSAGES.cli.catalog.as_str())]
    catalog: PathBuf,

    // All interfaces by default: the app is meant to be reached from the local network.
    #[arg(long, default_value = "0.0.0.0", help = MESSAGES.cli.host.as_str())]
    host: IpAddr,

    #[arg(long, default_value_t = 8080, help = MESSAGES.cli.port.as_str())]
    port: u16,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match serve(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(cli: Cli) -> anyhow::Result<()> {
    if !cli.catalog.is_dir() {
        bail!(interpolate(
            &MESSAGES.cli.catalog_not_a_directory,
            &[("path", &cli.catalog.display())],
        ));
    }

    let address = SocketAddr::new(cli.host, cli.port);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| interpolate(&MESSAGES.cli.bind_failed, &[("address", &address)]))?;

    println!(
        "{}",
        interpolate(
            &MESSAGES.cli.listening,
            &[("catalog", &cli.catalog.display()), ("address", &address)],
        )
    );

    axum::serve(listener, web::router())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}
