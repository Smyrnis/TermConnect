use super::*;

impl App {
    pub(super) fn open_transfers_screen(&mut self) {
        self.screen = Screen::Transfers;
        self.transfers_cursor = 0;
    }

    pub(super) fn transfer_rows(&self) -> &[QueueRow] {
        &self.transfers.rows
    }

    pub(super) fn apply_transfer_snapshot(&mut self, snapshot: TransferSnapshot) {
        self.transfers = snapshot;
        if let Some(kind) = self.reselect_row.take() {
            self.transfers_cursor = self
                .transfer_rows()
                .iter()
                .position(|row| row.kind == kind)
                .unwrap_or_else(|| self.clamped_transfers_cursor());
        }
    }

    pub(super) fn apply_transfers_action(&mut self, action: Action) {
        match action {
            Action::Up => self.transfers_cursor = self.clamped_transfers_cursor().saturating_sub(1),
            Action::Down => {
                let last = self.transfer_rows().len().saturating_sub(1);
                self.transfers_cursor = (self.clamped_transfers_cursor() + 1).min(last);
            }
            Action::Open => {
                if let Some(kind) = self.selected_row_kind() {
                    self.core.send(Command::RetryRow { kind });
                }
            }
            Action::Refresh => {
                if self.transfer_rows().is_empty() {
                    return;
                }
                self.reselect_row = self.selected_row_kind();
                self.core.send(Command::ClearFinished);
            }
            _ => {}
        }
    }

    pub(super) fn cancel_selected_row(&mut self) {
        if let Some(kind) = self.selected_row_kind() {
            self.core.send(Command::CancelRow { kind });
        }
    }

    fn selected_row_kind(&self) -> Option<RowKind> {
        self.transfer_rows().get(self.clamped_transfers_cursor()).map(|row| row.kind)
    }

    fn clamped_transfers_cursor(&self) -> usize {
        self.transfers_cursor.min(self.transfer_rows().len().saturating_sub(1))
    }
}

#[cfg(test)]
mod tests;
