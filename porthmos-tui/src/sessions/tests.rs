use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use super::*;

static NEXT_ID: AtomicU64 = AtomicU64::new(100);

fn add(sessions: &mut Sessions, name: &str) -> SessionId {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    sessions.insert(id, name.to_string(), true, panel())
}

fn panel() -> PanelView {
    PanelView::from_listing(PathBuf::from("/home/user"), Vec::new())
}

#[test]
fn insert_makes_the_new_session_active() {
    let mut sessions = Sessions::new();
    let id = add(&mut sessions, "a");

    assert_eq!(sessions.active().unwrap().id, id);
    assert_eq!(sessions.len(), 1);
}

#[test]
fn by_host_finds_a_session_by_connection_name() {
    let mut sessions = Sessions::new();
    add(&mut sessions, "production");

    assert!(sessions.by_name("production").is_some());
    assert!(sessions.by_name("staging").is_none());
}

#[test]
fn cycle_advances_through_sessions_and_wraps() {
    let mut sessions = Sessions::new();
    let a = add(&mut sessions, "a");
    let b = add(&mut sessions, "b");
    assert_eq!(sessions.active().unwrap().id, b);

    sessions.cycle();
    assert_eq!(sessions.active().unwrap().id, a);
    sessions.cycle();
    assert_eq!(sessions.active().unwrap().id, b);
}

#[test]
fn cycle_is_a_no_op_with_zero_or_one_sessions() {
    let mut sessions = Sessions::new();
    sessions.cycle();
    assert!(sessions.active().is_none());

    add(&mut sessions, "a");
    sessions.cycle();
    assert_eq!(sessions.len(), 1);
}

#[test]
fn removing_the_active_session_activates_the_next_one() {
    let mut sessions = Sessions::new();
    let a = add(&mut sessions, "a");
    let b = add(&mut sessions, "b");
    sessions.cycle();

    let removed = sessions.remove(a).unwrap();

    assert_eq!(removed.id, a);
    assert_eq!(sessions.active().unwrap().id, b);
}

#[test]
fn removing_the_last_session_leaves_nothing_active() {
    let mut sessions = Sessions::new();
    let a = add(&mut sessions, "a");

    sessions.remove(a);

    assert!(sessions.active().is_none());
    assert!(sessions.is_empty());
}

#[test]
fn removing_an_inactive_session_keeps_the_active_one_unchanged() {
    let mut sessions = Sessions::new();
    let a = add(&mut sessions, "a");
    let b = add(&mut sessions, "b");

    sessions.remove(a);

    assert_eq!(sessions.active().unwrap().id, b);
    assert_eq!(sessions.len(), 1);
}

#[test]
fn activate_switches_to_the_session_with_the_given_id() {
    let mut sessions = Sessions::new();
    let a = add(&mut sessions, "a");
    let _b = add(&mut sessions, "b");

    assert!(sessions.activate(a));
    assert_eq!(sessions.active().unwrap().id, a);
    assert!(!sessions.activate(999));
}
