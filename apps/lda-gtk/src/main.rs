//! GTK shell. Closing the window does not stop the daemon.

#![forbid(unsafe_code)]

mod bus;
mod markdown;
mod notes;
mod settings;
mod setup;
mod talk;
mod window;

use std::path::PathBuf;
use std::sync::OnceLock;

use adw::prelude::*;
use clap::Parser;
use gtk::glib;

use crate::bus::Launch;

#[derive(Parser)]
#[command(name = "lda-gtk", version, about = "Linux Desktop Agent")]
struct Cli {
    /// Configuration file passed to a daemon this process starts.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Data directory passed to a daemon this process starts.
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

fn main() -> glib::ExitCode {
    let cli = Cli::parse();
    let _ = PATHS.set(Launch {
        config: cli.config,
        data_dir: cli.data_dir,
    });
    let app = adw::Application::builder()
        .application_id(lda_common::GTK_APP_ID)
        .build();
    app.connect_activate(|app| activate(app.upcast_ref()));
    let argv = std::env::args().next().unwrap_or_else(|| "lda-gtk".into());
    app.run_with_args(&[&argv])
}

fn activate(app: &gtk::Application) {
    let launch = PATHS.get().cloned().unwrap_or(Launch {
        config: None,
        data_dir: None,
    });
    let (cmd_tx, cmd_rx) = async_channel::unbounded();
    let (ui_tx, ui_rx) = async_channel::unbounded();
    runtime().spawn(async move {
        bus::run(cmd_rx, ui_tx, launch).await;
    });
    let ctx = window::build(app, cmd_tx);
    window::bind_events(ctx, ui_rx);
}

fn runtime() -> &'static tokio::runtime::Handle {
    static HANDLE: OnceLock<tokio::runtime::Handle> = OnceLock::new();
    HANDLE.get_or_init(|| {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let runtime = Box::leak(Box::new(runtime));
        runtime.handle().clone()
    })
}

static PATHS: OnceLock<Launch> = OnceLock::new();
