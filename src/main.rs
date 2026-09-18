use anyhow::Result;

// File permissions (connection/store.rs, config/bookmarks.rs) and the F4
// terminal handoff (terminal.rs) both use std::os::unix APIs directly, so
// this only builds on Unix-like platforms — fail early with a clear
// message rather than a wall of missing-type errors on Windows.
#[cfg(not(unix))]
compile_error!("termconnect only supports Unix-like platforms (Linux/macOS)");

mod app;
mod config;
mod connection;
mod errors;
mod filesystem;
mod logging;
mod terminal;
mod transfer;
mod tui;

use app::App;

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = tui::restore();
        default_hook(panic_info);
    }));
}

/// Sends trace output to a log file rather than the default stdout writer,
/// which would print over the TUI's alternate screen the moment `RUST_LOG`
/// is set. Falls back to discarding log output (rather than stdout) if the
/// log file can't be opened, so a broken log path degrades quietly instead
/// of corrupting the display.
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::from_default_env();
    match logging::open_writer() {
        Ok(file) => {
            tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::sync::Mutex::new(file)).init();
        }
        Err(err) => {
            eprintln!("termconnect: failed to open log file, logging disabled: {err}");
            tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::sink).init();
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    install_panic_hook();

    let mut terminal = tui::init()?;

    let result = match App::new() {
        Ok(mut app) => app.run(&mut terminal).await,
        Err(err) => Err(err),
    };

    tui::restore()?;

    result
}
