use super::*;
use crate::connection::ConnectionSource;
use std::path::PathBuf;

fn entry(name: &str) -> ConnectionEntry {
    ConnectionEntry {
        name: name.to_string(),
        host: format!("{name}.example.com"),
        port: 22,
        username: "user".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
        source: ConnectionSource::Profile,
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
