//! User-level daemon. It does not need the GTK process, and it never runs as root.

#![forbid(unsafe_code)]

mod service;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use lda_common::{xdg_config_dir, xdg_data_dir, Error, Result};
use lda_ipc::serve_session;
use lda_security::KeyringSecretStore;
use tokio::signal::unix::{signal, SignalKind};
use tracing::{info, warn};

use service::Service;

#[derive(Parser)]
#[command(
    name = "lda-daemon",
    version,
    about = "Linux Desktop Agent background service"
)]
struct Args {
    /// Configuration file. Default: $XDG_CONFIG_HOME/lda/config.toml
    #[arg(long)]
    config: Option<PathBuf>,
    /// Data directory. Default: $XDG_DATA_HOME/lda
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("lda-daemon: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    init_logging();
    refuse_root()?;
    let args = Args::parse();
    let config_path = args
        .config
        .unwrap_or_else(|| xdg_config_dir().join("config.toml"));
    let data_dir = args.data_dir.unwrap_or_else(xdg_data_dir);
    let service = Service::open(config_path, data_dir, Arc::new(KeyringSecretStore::new()))?;
    let conn = match serve_session(service.clone()).await {
        Ok(conn) => conn,
        Err(zbus::Error::NameTaken) => {
            info!("another lda-daemon already owns the session name");
            return Ok(());
        }
        Err(err) => return Err(Error::Ipc(err.to_string())),
    };
    service.signals().attach(conn.clone());
    info!("listening on the session bus");
    wait_for_stop(service.as_ref()).await;
    service.request_stop();
    tokio::time::sleep(Duration::from_millis(300)).await;
    tokio::select! {
        _ = conn.graceful_shutdown() => {}
        _ = tokio::time::sleep(Duration::from_secs(2)) => {
            warn!("bus shutdown timed out");
        }
    }
    info!("stopped");
    Ok(())
}

async fn wait_for_stop(service: &Service) {
    let term = signal(SignalKind::terminate());
    let mut term = match term {
        Ok(term) => term,
        Err(err) => {
            warn!(%err, "SIGTERM could not be hooked");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = service.stopped() => {}
            }
            return;
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
        _ = service.stopped() => {}
    }
}

fn refuse_root() -> Result<()> {
    let text = std::fs::read_to_string("/proc/self/status")?;
    let Some(uid) = text.lines().find_map(uid_field) else {
        return Err(Error::PermissionDenied(
            "could not read the daemon uid".into(),
        ));
    };
    if uid == 0 {
        return Err(Error::PermissionDenied(
            "the daemon refuses to run as root".into(),
        ));
    }
    Ok(())
}

fn uid_field(line: &str) -> Option<u32> {
    let rest = line.strip_prefix("Uid:")?;
    rest.split_whitespace().next()?.parse().ok()
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,zbus=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

#[cfg(test)]
mod tests {
    use super::uid_field;

    #[test]
    fn uid_field_reads_the_first_number() {
        assert_eq!(uid_field("Uid:\t1000\t1000\t1000\t1000"), Some(1000));
        assert_eq!(uid_field("Name:\tdaemon"), None);
    }
}
