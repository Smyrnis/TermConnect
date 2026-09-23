use anyhow::Result;

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
