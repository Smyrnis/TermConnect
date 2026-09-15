use std::io::{self, Stdout};

use anyhow::Result;
use crossterm::cursor::Show;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub mod input;
pub mod layout;
pub mod panels;
pub mod sort;
pub mod widgets;

pub type Backend = CrosstermBackend<Stdout>;

pub fn init() -> Result<Terminal<Backend>> {
    enable_raw_mode()?;

    if let Err(err) = execute!(io::stdout(), EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(err.into());
    }

    match Terminal::new(CrosstermBackend::new(io::stdout())) {
        Ok(terminal) => Ok(terminal),
        Err(err) => {
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            let _ = disable_raw_mode();
            Err(err.into())
        }
    }
}

pub fn restore() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, Show)?;
    Ok(())
}
