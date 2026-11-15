use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::Parser;

use on_mange_quoi::catalog::{self, Catalog, Problem};
use on_mange_quoi::i18n::{MESSAGES, interpolate};
use on_mange_quoi::live::{self, LiveCatalog, Snapshot};
use on_mange_quoi::{report, web};

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

    #[arg(
        long,
        value_name = "FILE",
        help = MESSAGES.cli.validate.as_str(),
        conflicts_with_all = ["host", "port"],
    )]
    validate: Vec<PathBuf>,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = if cli.validate.is_empty() {
        serve(cli).await
    } else {
        validate(&cli)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

fn validate(cli: &Cli) -> anyhow::Result<()> {
    let catalog = load_catalog(&cli.catalog, &cli.validate)?;
    println!("{}", summary(&MESSAGES.cli.catalog_valid, &catalog));
    Ok(())
}

async fn serve(cli: Cli) -> anyhow::Result<()> {
    let fingerprint = catalog::fingerprint(&cli.catalog);
    let catalog = load_catalog(&cli.catalog, &[])?;
    println!("{}", summary(&MESSAGES.cli.catalog_loaded, &catalog));

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

    let live = Arc::new(LiveCatalog::new(cli.catalog.clone(), catalog));
    live::spawn_watcher(Arc::clone(&live), fingerprint, RELOAD_INTERVAL, log_reload);

    axum::serve(listener, web::router(live))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// How often the catalog directory is checked for changes.
const RELOAD_INTERVAL: Duration = Duration::from_secs(1);

fn log_reload(snapshot: &Snapshot) {
    if snapshot.problems.is_empty() {
        println!(
            "{}",
            summary(&MESSAGES.cli.catalog_reloaded, &snapshot.catalog)
        );
        return;
    }
    for problem in snapshot.problems.iter() {
        eprintln!("{}", report::describe(problem));
    }
    eprintln!(
        "{}",
        interpolate(
            &MESSAGES.cli.catalog_reload_rejected,
            &[("count", &snapshot.problems.len())],
        )
    );
}

/// Loads the catalog with `extra_files` on top; on failure, prints every problem.
fn load_catalog(dir: &Path, extra_files: &[PathBuf]) -> anyhow::Result<Catalog> {
    if !dir.is_dir() {
        bail!(interpolate(
            &MESSAGES.cli.catalog_not_a_directory,
            &[("path", &dir.display())],
        ));
    }
    catalog::load_with(dir, extra_files).or_else(|problems: Vec<Problem>| {
        for problem in &problems {
            eprintln!("{}", report::describe(problem));
        }
        bail!(interpolate(
            &MESSAGES.cli.catalog_invalid,
            &[("count", &problems.len())],
        ))
    })
}

fn summary(template: &str, catalog: &Catalog) -> String {
    interpolate(
        template,
        &[
            ("recipes", &catalog.recipes.len()),
            ("ingredients", &catalog.ingredients.len()),
            ("tags", &catalog.tags.len()),
        ],
    )
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
