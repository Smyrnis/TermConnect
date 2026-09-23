pub mod confirm;
pub mod form;
pub mod list;
pub mod text_input;

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;

pub use confirm::ConfirmDialog;
pub use form::{FormDialog, FormField};
pub use list::ListDialog;
pub use text_input::TextInputDialog;

fn content_width(lines: &[&str]) -> u16 {
    let longest = lines.iter().map(|line| line.chars().count()).max().unwrap_or(0);
    (longest as u16 + 4).clamp(20, 76)
}

pub enum Dialog {
    Confirm(ConfirmDialog),
    TextInput(TextInputDialog),
    List(ListDialog),
    Form(FormDialog),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOutcome {
    Pending,
    Confirmed,
    Cancelled,
    Submitted(String),
    Selected(usize),
    Removed(usize),
    FormSubmitted(Vec<String>),
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
            Dialog::List(dialog) => match dialog.handle_key(key) {
                list::ListOutcome::Pending => DialogOutcome::Pending,
                list::ListOutcome::Selected(index) => DialogOutcome::Selected(index),
                list::ListOutcome::Removed(index) => DialogOutcome::Removed(index),
                list::ListOutcome::Cancelled => DialogOutcome::Cancelled,
            },
            Dialog::Form(dialog) => match dialog.handle_key(key) {
                form::FormOutcome::Pending => DialogOutcome::Pending,
                form::FormOutcome::Submitted(values) => DialogOutcome::FormSubmitted(values),
                form::FormOutcome::Cancelled => DialogOutcome::Cancelled,
            },
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        match self {
            Dialog::Confirm(dialog) => confirm::render_confirm(frame, area, dialog),
            Dialog::TextInput(dialog) => text_input::render_text_input(frame, area, dialog),
            Dialog::List(dialog) => list::render_list(frame, area, dialog),
            Dialog::Form(dialog) => form::render_form(frame, area, dialog),
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/tui/dialog/mod_test.rs"]
mod tests;
