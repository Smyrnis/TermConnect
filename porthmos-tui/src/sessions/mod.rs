use porthmos_core::SessionId;

use crate::widgets::panel_view::PanelView;

pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub shell_available: bool,
    pub panel: PanelView,
}

#[derive(Default)]
pub struct Sessions {
    items: Vec<Session>,
    active: Option<usize>,
}

impl Sessions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active(&self) -> Option<&Session> {
        self.active.and_then(|index| self.items.get(index))
    }

    pub fn active_mut(&mut self) -> Option<&mut Session> {
        self.active.and_then(move |index| self.items.get_mut(index))
    }

    pub fn active_id(&self) -> Option<SessionId> {
        self.active().map(|session| session.id)
    }

    pub fn by_id(&self, id: SessionId) -> Option<&Session> {
        self.items.iter().find(|session| session.id == id)
    }

    pub fn by_id_mut(&mut self, id: SessionId) -> Option<&mut Session> {
        self.items.iter_mut().find(|session| session.id == id)
    }

    pub fn by_name(&self, name: &str) -> Option<&Session> {
        self.items.iter().find(|session| session.name == name)
    }

    pub fn insert(&mut self, id: SessionId, name: String, shell_available: bool, panel: PanelView) -> SessionId {
        self.items.push(Session { id, name, shell_available, panel });
        self.active = Some(self.items.len() - 1);
        id
    }

    pub fn remove(&mut self, id: SessionId) -> Option<Session> {
        let index = self.items.iter().position(|session| session.id == id)?;
        let removed = self.items.remove(index);

        self.active = match self.active {
            Some(active) if active == index => {
                if self.items.is_empty() {
                    None
                } else {
                    Some(active.min(self.items.len() - 1))
                }
            }
            Some(active) if active > index => Some(active - 1),
            other => other,
        };

        Some(removed)
    }

    pub fn cycle(&mut self) {
        if self.items.len() < 2 {
            return;
        }
        self.active = Some((self.active.unwrap_or(0) + 1) % self.items.len());
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Session> {
        self.items.iter()
    }

    pub fn activate(&mut self, id: SessionId) -> bool {
        if let Some(index) = self.items.iter().position(|session| session.id == id) {
            self.active = Some(index);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests;
