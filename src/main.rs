use anyhow::Result;

mod app;
mod tui;

use app::App;

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = tui::restore();
        default_hook(panic_info);
    }));
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    install_panic_hook();

    let mut terminal = tui::init()?;
    let result = App::new().run(&mut terminal).await;

    tui::restore()?;

    result
}
