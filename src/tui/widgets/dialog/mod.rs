pub mod confirm;
pub mod text_input;

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;

pub use confirm::ConfirmDialog;
pub use text_input::TextInputDialog;

pub enum Dialog {
    Confirm(ConfirmDialog),
    TextInput(TextInputDialog),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOutcome {
    Pending,
    Confirmed,
    Cancelled,
    Submitted(String),
}

impl Dialog {
    pub fn handle_key(&mut self, key: KeyEvent) -> DialogOutcome {
        match self {
            Dialog::Confirm(dialog) => match dialog.handle_key(key) {
                confirm::ConfirmOutcome::Pending => DialogOutcome::Pending,
                confirm::ConfirmOutcome::Confirmed => DialogOutcome::Confirmed,
                confirm::ConfirmOutcome::Cancelled => DialogOutcome::Cancelled,
            },
            Dialog::TextInput(dialog) => match dialog.handle_key(key) {
                text_input::TextInputOutcome::Pending => DialogOutcome::Pending,
                text_input::TextInputOutcome::Submitted(value) => DialogOutcome::Submitted(value),
                text_input::TextInputOutcome::Cancelled => DialogOutcome::Cancelled,
            },
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        match self {
            Dialog::Confirm(dialog) => confirm::render_confirm(frame, area, dialog),
            Dialog::TextInput(dialog) => text_input::render_text_input(frame, area, dialog),
        }
    }
}
