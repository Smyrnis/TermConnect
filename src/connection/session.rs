use crate::connection::ConnectionEntry;
use crate::tui::panels::PanelState;

pub struct Session {
    pub id: u64,
    pub entry: ConnectionEntry,
    pub panel: PanelState,
}

#[derive(Default)]
pub struct Sessions {
    items: Vec<Session>,
    active: Option<usize>,
    next_id: u64,
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

    pub fn active_id(&self) -> Option<u64> {
        self.active().map(|session| session.id)
    }

    pub fn by_id_mut(&mut self, id: u64) -> Option<&mut Session> {
        self.items.iter_mut().find(|session| session.id == id)
    }

    pub fn by_host(&self, name: &str) -> Option<&Session> {
        self.items.iter().find(|session| session.entry.name == name)
    }

    /// Adds a new session and makes it the active one.
    pub fn insert(&mut self, entry: ConnectionEntry, panel: PanelState) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Session { id, entry, panel });
        self.active = Some(self.items.len() - 1);
        id
    }

    /// Removes the session with `id`. If it was active, the next session
    /// (or the previous one, if it was last) becomes active; if it was the
    /// only session, nothing is active afterward.
    pub fn remove(&mut self, id: u64) -> Option<Session> {
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

    /// Advances to the next session, wrapping around. A no-op with zero or
    /// one sessions.
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

    /// Makes the session with `id` active, if it exists. Returns whether
    /// it was found.
    pub fn activate(&mut self, id: u64) -> bool {
        if let Some(index) = self.items.iter().position(|session| session.id == id) {
            self.active = Some(index);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(name: &str) -> ConnectionEntry {
        ConnectionEntry {
            name: name.to_string(),
            host: format!("{name}.example.com"),
            port: 22,
            username: "user".to_string(),
            identity_file: None,
        }
    }

    fn panel() -> PanelState {
        PanelState::from_listing(PathBuf::from("/home/user"), Vec::new())
    }

    #[test]
    fn insert_makes_the_new_session_active() {
        let mut sessions = Sessions::new();
        let id = sessions.insert(entry("a"), panel());

        assert_eq!(sessions.active().unwrap().id, id);
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn by_host_finds_a_session_by_connection_name() {
        let mut sessions = Sessions::new();
        sessions.insert(entry("production"), panel());

        assert!(sessions.by_host("production").is_some());
        assert!(sessions.by_host("staging").is_none());
    }

    #[test]
    fn cycle_advances_through_sessions_and_wraps() {
        let mut sessions = Sessions::new();
        let a = sessions.insert(entry("a"), panel());
        let b = sessions.insert(entry("b"), panel());
        assert_eq!(sessions.active().unwrap().id, b); // insert activates the newest

        sessions.cycle();
        assert_eq!(sessions.active().unwrap().id, a);
        sessions.cycle();
        assert_eq!(sessions.active().unwrap().id, b);
    }

    #[test]
    fn cycle_is_a_no_op_with_zero_or_one_sessions() {
        let mut sessions = Sessions::new();
        sessions.cycle(); // zero sessions
        assert!(sessions.active().is_none());

        sessions.insert(entry("a"), panel());
        sessions.cycle(); // one session
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn removing_the_active_session_activates_the_next_one() {
        let mut sessions = Sessions::new();
        let a = sessions.insert(entry("a"), panel());
        let b = sessions.insert(entry("b"), panel());
        sessions.cycle(); // active is now `a`

        let removed = sessions.remove(a).unwrap();

        assert_eq!(removed.id, a);
        assert_eq!(sessions.active().unwrap().id, b);
    }

    #[test]
    fn removing_the_last_session_leaves_nothing_active() {
        let mut sessions = Sessions::new();
        let a = sessions.insert(entry("a"), panel());

        sessions.remove(a);

        assert!(sessions.active().is_none());
        assert!(sessions.is_empty());
    }

    #[test]
    fn removing_an_inactive_session_keeps_the_active_one_unchanged() {
        let mut sessions = Sessions::new();
        let a = sessions.insert(entry("a"), panel());
        let b = sessions.insert(entry("b"), panel()); // active

        sessions.remove(a);

        assert_eq!(sessions.active().unwrap().id, b);
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn activate_switches_to_the_session_with_the_given_id() {
        let mut sessions = Sessions::new();
        let a = sessions.insert(entry("a"), panel());
        let _b = sessions.insert(entry("b"), panel());

        assert!(sessions.activate(a));
        assert_eq!(sessions.active().unwrap().id, a);
        assert!(!sessions.activate(999));
    }
}
