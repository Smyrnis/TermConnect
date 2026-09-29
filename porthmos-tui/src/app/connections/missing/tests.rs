use std::path::Path;

use porthmos_core::{
    Command, Event,
    profiles::{ConnectionEntry, ConnectionSource, Labels},
};

use crate::{app::testing::test_app, widgets::dialog::Dialog};

fn missing(name: &str) -> ConnectionEntry {
    ConnectionEntry::orphan_labels(
        name.to_string(),
        Labels { group: None, tags: vec!["t".into()] },
        ConnectionSource::MissingSshHost,
    )
}

fn host(name: &str, labelled: bool) -> ConnectionEntry {
    let mut entry = missing(name);
    entry.source = ConnectionSource::SshConfig;
    entry.host = format!("{name}.example");
    if !labelled {
        entry.tags.clear();
    }
    entry
}

#[test]
fn missing_hosts_are_announced_once() {
    let mut test = test_app(Path::new("/d"));

    test.app.apply_core_event(Event::Profiles(vec![missing("web1"), missing("db2")]));
    let Some(Dialog::Message(message)) = &test.app.dialog else { panic!("no announcement") };
    assert_eq!(message.title, "Labelled ssh hosts");
    assert_eq!(
        message.message,
        "These labelled hosts are no longer in ~/.ssh/config: db2, web1. They are marked \u{26A0} in the connection list."
    );
    test.app.dialog = None;

    test.app.apply_core_event(Event::Profiles(vec![missing("web1"), missing("db2")]));
    assert!(test.app.dialog.is_none());
}

#[test]
fn the_announcement_waits_while_another_dialog_is_open() {
    let mut test = test_app(Path::new("/d"));
    test.app.dialog = Some(Dialog::Confirm(crate::widgets::dialog::ConfirmDialog::new("busy")));

    test.app.apply_core_event(Event::Profiles(vec![missing("web1")]));
    assert!(matches!(test.app.dialog, Some(Dialog::Confirm(_))));

    test.app.dialog = None;
    test.app.apply_core_event(Event::Profiles(vec![missing("web1")]));
    assert!(matches!(test.app.dialog, Some(Dialog::Message(_))));
}

#[test]
fn forget_sends_forget_labels() {
    let mut test = test_app(Path::new("/d"));
    test.app.announced_missing.insert("web1".into());
    test.app.connections.replace(vec![missing("web1")]);

    test.app.open_missing_host_dialog();
    test.app.apply_missing_host_choice("web1".into(), 1);

    assert_eq!(test.sent(), vec![Command::ForgetSshLabels { name: "web1".into() }]);
}

#[test]
fn move_offers_unlabelled_hosts_and_sends_the_pick() {
    let mut test = test_app(Path::new("/d"));
    test.app.announced_missing.insert("old".into());
    test.app.connections.replace(vec![missing("old"), host("labelled", true), host("new", false)]);

    test.app.apply_missing_host_choice("old".into(), 0);
    let Some(Dialog::List(list)) = &test.app.dialog else { panic!("no host list") };
    assert_eq!(list.items, vec!["new".to_string()]);

    test.app.move_labels_to("old".into(), vec!["new".into()], 0);
    assert_eq!(test.sent(), vec![Command::MoveSshLabels { from: "old".into(), to: "new".into() }]);
}

#[test]
fn move_without_candidates_shows_a_notice() {
    let mut test = test_app(Path::new("/d"));
    test.app.announced_missing.insert("old".into());
    test.app.connections.replace(vec![missing("old")]);

    test.app.apply_missing_host_choice("old".into(), 0);

    assert!(test.app.dialog.is_none());
    assert_eq!(test.notification().as_deref(), Some("No unlabelled ssh hosts to move the labels to"));
}

#[test]
fn enter_on_a_missing_row_opens_the_fix_dialog() {
    let mut test = test_app(Path::new("/d"));
    test.app.announced_missing.insert("web1".into());
    test.app.screen = crate::app::Screen::Connections;
    test.app.connections.replace(vec![missing("web1")]);

    test.app.apply_action(crate::input::Action::Open);

    let Some(Dialog::List(list)) = &test.app.dialog else { panic!("no fix dialog") };
    assert_eq!(list.title, "web1 is no longer in ~/.ssh/config");
    assert_eq!(list.items, vec!["Move labels to\u{2026}", "Forget labels", "Cancel"]);
    assert!(test.sent().is_empty());
}

#[test]
fn choosing_from_the_fix_list_by_key_reaches_the_choice() {
    let mut test = test_app(Path::new("/d"));
    test.app.announced_missing.insert("web1".into());
    test.app.screen = crate::app::Screen::Connections;
    test.app.connections.replace(vec![missing("web1")]);
    test.app.apply_action(crate::input::Action::Open);

    test.app.apply_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Down,
        crossterm::event::KeyModifiers::NONE,
    ));
    test.app.apply_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));

    assert_eq!(test.sent(), vec![Command::ForgetSshLabels { name: "web1".into() }]);
}

fn shadowed(name: &str) -> ConnectionEntry {
    let mut entry = missing(name);
    entry.source = ConnectionSource::ShadowedSshHost;
    entry
}

fn press(test: &mut crate::app::testing::TestApp, code: crossterm::event::KeyCode) {
    test.app.apply_key(crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE));
}

#[test]
fn the_announcement_appears_once_the_blocking_dialog_closes() {
    let mut test = test_app(Path::new("/d"));
    test.app.dialog = Some(Dialog::Confirm(crate::widgets::dialog::ConfirmDialog::new("busy")));
    test.app.apply_core_event(Event::Profiles(vec![missing("web1")]));

    press(&mut test, crossterm::event::KeyCode::Esc);

    let Some(Dialog::Message(message)) = &test.app.dialog else { panic!("no announcement after closing") };
    assert!(message.message.contains("web1"), "{}", message.message);
}

#[test]
fn shadowed_hosts_get_their_own_sentence_and_title() {
    let mut test = test_app(Path::new("/d"));

    test.app.apply_core_event(Event::Profiles(vec![missing("old"), shadowed("web1")]));

    let Some(Dialog::Message(message)) = &test.app.dialog else { panic!("no announcement") };
    assert_eq!(
        message.message,
        "These labelled hosts are no longer in ~/.ssh/config: old. These labelled hosts are hidden by a saved \
         connection of the same name: web1. They are marked \u{26A0} in the connection list."
    );
    test.app.dialog = None;
    test.app.screen = crate::app::Screen::Connections;
    test.app.connections.cursor = 1;

    test.app.apply_action(crate::input::Action::Open);

    let Some(Dialog::List(list)) = &test.app.dialog else { panic!("no fix dialog") };
    assert_eq!(list.title, "web1 is hidden by a saved connection");
}

#[test]
fn closing_the_announcement_does_not_announce_again() {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_core_event(Event::Profiles(vec![missing("web1")]));

    press(&mut test, crossterm::event::KeyCode::Enter);

    assert!(test.app.dialog.is_none());
}
