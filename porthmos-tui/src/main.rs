use anyhow::Result;

#[cfg(not(unix))]
compile_error!("porthmos only supports Unix-like platforms (Linux/macOS)");

mod app;
mod input;
mod logging;
mod sessions;
mod terminal;
mod widgets;

use app::App;
use porthmos_core::{Core, Environment, Paths, config};

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = terminal::restore();
        default_hook(panic_info);
    }));
}

fn init_tracing(log_file: Option<&std::path::Path>) {
    let filter = tracing_subscriber::EnvFilter::from_default_env();
    let writer = match log_file {
        Some(path) => logging::open_writer_at(path),
        None => Err(anyhow::anyhow!("HOME environment variable is not set")),
    };
    match writer {
        Ok(file) => {
            tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::sync::Mutex::new(file)).init();
        }
        Err(err) => {
            eprintln!("porthmos: failed to open log file, logging disabled: {err}");
            tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::sink).init();
        }
    }
}

fn start_app(
    env: Environment, paths: Paths,
) -> Result<(App, tokio::sync::mpsc::UnboundedReceiver<porthmos_core::Event>)> {
    let (settings, config_warnings) = config::load(&paths)?;
    let (key_bindings, key_warnings) = input::KeyBindings::from_frontend(&settings.frontend);
    let panel = settings.panel.clone();
    let (core, events) = Core::builder().paths(paths).environment(env).settings(settings).start()?;

    let mut app = App::new(core, std::env::current_dir()?, &panel, key_bindings);
    for warning in config_warnings {
        app.warn(warning.0);
    }
    for warning in key_warnings {
        app.warn(warning);
    }
    Ok((app, events))
}

#[tokio::main]
async fn main() -> Result<()> {
    let env = Environment::from_process();
    let paths = Paths::from_env(&env);
    init_tracing(paths.as_ref().ok().map(Paths::log_file).as_deref());

    install_panic_hook();

    let mut terminal = terminal::init()?;

    let result = match paths.and_then(|paths| start_app(env, paths)) {
        Ok((mut app, events)) => app.run(&mut terminal, events).await,
        Err(err) => Err(err),
    };

    terminal::restore()?;

    result
}
