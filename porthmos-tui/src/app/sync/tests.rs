use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use porthmos_core::sync::{SyncAction, SyncFacts, SyncItem, SyncReason};

use super::*;
use crate::{
    app::testing::{TestApp, test_app},
    widgets::dialog::FieldKind,
};

fn connected_without_listing(preserves_times: bool) -> TestApp {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_core_event(Event::Connected { session: 3, name: "prod".to_string(), shell_available: false });
    test.app.apply_core_event(Event::SessionCapabilities { session: 3, preserves_times });
    test
}

fn connected(preserves_times: bool) -> TestApp {
    let mut test = connected_without_listing(preserves_times);
    test.app.apply_core_event(Event::Listed {
        location: Location::Session(3),
        path: PathBuf::from("/srv"),
        entries: Vec::new(),
    });
    test.sent();
    test
}

fn press(test: &mut TestApp, code: KeyCode) {
    test.app.apply_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn open(test: &mut TestApp) {
    test.app.apply_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
}

fn form(test: &TestApp) -> &FormDialog {
    match &test.app.dialog {
        Some(Dialog::Form(form)) => form,
        _ => panic!("expected the sync options form"),
    }
}

fn choice_values(form: &FormDialog, key: &str) -> Vec<String> {
    match &form.fields.iter().find(|field| field.key == key).unwrap().kind {
        FieldKind::Choice { choices, .. } => choices.iter().map(|(value, _)| value.clone()).collect(),
        _ => panic!("not a choice"),
    }
}

#[test]
fn without_a_connected_remote_the_key_only_warns() {
    let mut test = test_app(Path::new("/d"));

    open(&mut test);

    assert!(test.app.dialog.is_none());
    assert_eq!(test.app.notifications.current().map(|note| note.severity), Some(Severity::Warning));
    assert!(test.sent().is_empty());
}

#[test]
fn the_dialog_offers_both_when_the_connection_keeps_times() {
    let mut test = connected(true);

    open(&mut test);

    assert_eq!(choice_values(form(&test), "direction"), ["local_to_remote", "remote_to_local", "both"]);
    assert_eq!(choice_values(form(&test), "by"), ["time", "size"]);
    assert_eq!(choice_values(form(&test), "subfolders"), ["yes", "no"]);
    assert!(form(&test).title.contains("prod"));
}

#[test]
fn the_dialog_leaves_out_both_and_explains_when_the_connection_cannot_keep_times() {
    let mut test = connected(false);

    open(&mut test);

    assert_eq!(choice_values(form(&test), "direction"), ["local_to_remote", "remote_to_local"]);
    assert!(form(&test).hint.as_deref().is_some_and(|hint| hint.contains("modification times")));
}

#[test]
fn enter_with_the_defaults_starts_a_local_to_remote_sync_of_the_two_panel_folders() {
    let mut test = connected(true);
    open(&mut test);

    press(&mut test, KeyCode::Enter);

    assert!(test.app.dialog.is_none() && test.app.pending_action.is_none());
    assert_eq!(
        test.sent(),
        vec![Command::StartSync {
            session: 3,
            local_dir: PathBuf::from("/d"),
            remote_dir: PathBuf::from("/srv"),
            options: SyncOptions { direction: SyncDirection::LocalToRemote, by: SyncBy::Time, subfolders: true },
        }]
    );
}

#[test]
fn every_field_can_be_changed_with_left_and_right() {
    let mut test = connected(true);
    open(&mut test);

    press(&mut test, KeyCode::Right);
    press(&mut test, KeyCode::Tab);
    press(&mut test, KeyCode::Right);
    press(&mut test, KeyCode::Tab);
    press(&mut test, KeyCode::Right);
    press(&mut test, KeyCode::Enter);

    assert_eq!(
        test.sent(),
        vec![Command::StartSync {
            session: 3,
            local_dir: PathBuf::from("/d"),
            remote_dir: PathBuf::from("/srv"),
            options: SyncOptions { direction: SyncDirection::RemoteToLocal, by: SyncBy::Size, subfolders: false },
        }]
    );
}

#[test]
fn both_with_size_is_refused_inside_the_dialog() {
    let mut test = connected(true);
    open(&mut test);
    press(&mut test, KeyCode::Right);
    press(&mut test, KeyCode::Right);
    press(&mut test, KeyCode::Tab);
    press(&mut test, KeyCode::Right);

    press(&mut test, KeyCode::Enter);

    assert!(form(&test).error.as_deref().is_some_and(|error| error.contains("size")));
    assert!(test.sent().is_empty());
}

#[test]
fn escape_cancels_without_sending_anything() {
    let mut test = connected(true);
    open(&mut test);

    press(&mut test, KeyCode::Esc);

    assert!(test.app.dialog.is_none() && test.app.pending_action.is_none());
    assert!(test.sent().is_empty());
}

#[test]
fn the_key_does_nothing_outside_the_file_screen() {
    let mut test = connected(true);
    test.app.screen = Screen::History;

    open(&mut test);

    assert!(test.app.dialog.is_none());
}

#[test]
fn session_capabilities_are_remembered_per_session() {
    let mut test = connected(false);

    test.app.apply_core_event(Event::SessionCapabilities { session: 3, preserves_times: true });

    assert!(test.app.sessions.by_id(3).unwrap().preserves_times);
}

fn values(direction: &str, by: &str, subfolders: &str) -> Vec<(&'static str, String)> {
    vec![("direction", direction.to_string()), ("by", by.to_string()), ("subfolders", subfolders.to_string())]
}

#[test]
fn a_session_that_disappears_while_the_dialog_is_open_warns_and_sends_nothing() {
    let mut test = connected(true);
    open(&mut test);
    test.app.apply_core_event(Event::Disconnected { session: 3, name: "prod".to_string() });

    press(&mut test, KeyCode::Enter);

    assert!(test.app.dialog.is_none() && test.app.pending_action.is_none());
    assert_eq!(test.app.notifications.current().map(|note| note.severity), Some(Severity::Warning));
    assert!(test.notification().is_some_and(|message| message.contains("no longer available")));
    assert!(test.sent().is_empty());
}

#[test]
fn before_the_first_listing_arrives_the_key_warns_instead_of_syncing_the_placeholder_folder() {
    let mut test = connected_without_listing(true);

    open(&mut test);

    assert!(test.app.dialog.is_none());
    assert_eq!(test.app.notifications.current().map(|note| note.severity), Some(Severity::Warning));
    assert!(test.sent().is_empty());

    test.app.apply_core_event(Event::Listed {
        location: Location::Session(3),
        path: PathBuf::from("/srv"),
        entries: Vec::new(),
    });
    open(&mut test);

    assert!(matches!(test.app.dialog, Some(Dialog::Form(_))));
}

#[test]
fn unknown_choice_values_are_refused_instead_of_replaced_by_defaults() {
    assert!(parse_options(&values("sideways", "time", "yes")).is_err());
    assert!(parse_options(&values("both", "weight", "yes")).is_err());
    assert!(parse_options(&values("both", "time", "maybe")).is_err());
    assert!(parse_options(&[("by", "time".to_string())]).is_err());
}

#[test]
fn every_known_choice_value_is_parsed() {
    assert_eq!(
        parse_options(&values("remote_to_local", "size", "no")),
        Ok(SyncOptions { direction: SyncDirection::RemoteToLocal, by: SyncBy::Size, subfolders: false })
    );
    assert_eq!(
        parse_options(&values("both", "time", "yes")),
        Ok(SyncOptions { direction: SyncDirection::Both, by: SyncBy::Time, subfolders: true })
    );
    assert!(parse_options(&values("both", "size", "yes")).is_err());
}

#[test]
fn a_hand_made_submission_with_an_unknown_value_keeps_the_dialog_open_with_an_error() {
    let mut test = connected(true);
    open(&mut test);

    test.app.submit_sync_options(3, values("sideways", "time", "yes"));

    assert!(form(&test).error.is_some());
    assert!(test.sent().is_empty());
}

#[test]
fn the_active_session_is_the_one_that_gets_synced() {
    let mut test = connected(true);
    test.connect(4, "stage");
    test.list_remote(4, "/other", Vec::new());
    test.sent();

    open(&mut test);
    press(&mut test, KeyCode::Enter);

    assert!(
        matches!(test.sent().as_slice(), [Command::StartSync { session: 4, remote_dir, .. }] if remote_dir == Path::new("/other"))
    );

    test.app.sessions.activate(3);
    open(&mut test);
    press(&mut test, KeyCode::Enter);

    assert!(
        matches!(test.sent().as_slice(), [Command::StartSync { session: 3, remote_dir, .. }] if remote_dir == Path::new("/srv"))
    );
}

#[test]
fn shift_tab_moves_back_to_the_previous_field() {
    let mut test = connected(true);
    open(&mut test);

    press(&mut test, KeyCode::Tab);
    press(&mut test, KeyCode::Right);
    press(&mut test, KeyCode::BackTab);
    press(&mut test, KeyCode::Right);
    press(&mut test, KeyCode::Enter);

    assert_eq!(
        test.sent(),
        vec![Command::StartSync {
            session: 3,
            local_dir: PathBuf::from("/d"),
            remote_dir: PathBuf::from("/srv"),
            options: SyncOptions { direction: SyncDirection::RemoteToLocal, by: SyncBy::Size, subfolders: true },
        }]
    );
}

fn plan_with(items: Vec<SyncItem>) -> Arc<SyncPlan> {
    plan_with_id(11, items)
}

fn plan_with_id(sync_id: u64, items: Vec<SyncItem>) -> Arc<SyncPlan> {
    Arc::new(SyncPlan {
        sync_id,
        session: 3,
        local_root: PathBuf::from("/d"),
        remote_root: PathBuf::from("/srv"),
        options: SyncOptions { direction: SyncDirection::LocalToRemote, by: SyncBy::Time, subfolders: true },
        items,
        skipped_symlinks: 0,
    })
}

fn upload(id: u32, path: &str) -> SyncItem {
    SyncItem {
        id,
        path: PathBuf::from(path),
        local: Some(SyncFacts { size: 5, modified: Some(10) }),
        remote: None,
        action: SyncAction::Upload,
        reason: SyncReason::OnlyLocal,
        ticked: true,
        flippable: false,
    }
}

fn newer_on_target(id: u32, path: &str) -> SyncItem {
    SyncItem {
        id,
        path: PathBuf::from(path),
        local: Some(SyncFacts { size: 5, modified: Some(10) }),
        remote: Some(SyncFacts { size: 5, modified: Some(90) }),
        action: SyncAction::Skip,
        reason: SyncReason::TargetNewer,
        ticked: false,
        flippable: true,
    }
}

fn with_plan(items: Vec<SyncItem>) -> TestApp {
    let mut test = connected(true);
    test.app.apply_core_event(Event::SyncPlanReady(plan_with(items)));
    test.sent();
    test
}

#[test]
fn a_plan_that_arrives_on_the_file_screen_opens_the_preview() {
    let mut test = connected(true);

    test.app.apply_core_event(Event::SyncPlanReady(plan_with(vec![upload(0, "a.txt")])));

    assert_eq!(test.app.screen, Screen::Sync);
    assert!(test.app.sync_view.has_plan());
}

#[test]
fn a_plan_that_arrives_elsewhere_waits_and_says_so() {
    let mut test = connected(true);
    test.app.screen = Screen::History;

    test.app.apply_core_event(Event::SyncPlanReady(plan_with(vec![upload(0, "a.txt")])));

    assert_eq!(test.app.screen, Screen::History);
    assert!(test.app.sync_view.has_plan());
    assert!(test.app.notifications.current().is_some_and(|note| note.message.contains("Ctrl+U")));
}

#[test]
fn the_key_reopens_a_waiting_plan_instead_of_asking_again() {
    let mut test = connected(true);
    test.app.screen = Screen::History;
    test.app.apply_core_event(Event::SyncPlanReady(plan_with(vec![upload(0, "a.txt")])));
    test.app.screen = Screen::Files;

    open(&mut test);

    assert_eq!(test.app.screen, Screen::Sync);
    assert!(test.app.dialog.is_none());
}

#[test]
fn space_toggles_and_a_n_act_on_the_rows() {
    let mut test = with_plan(vec![upload(0, "a.txt"), upload(1, "b.txt")]);

    press(&mut test, KeyCode::Char(' '));
    assert_eq!(test.app.sync_view.choices(), vec![(1, SyncAction::Upload)]);
    press(&mut test, KeyCode::Char('a'));
    assert_eq!(test.app.sync_view.choices().len(), 2);
    press(&mut test, KeyCode::Char('n'));
    assert!(test.app.sync_view.choices().is_empty());
}

#[test]
fn up_and_down_move_between_rows() {
    let mut test = with_plan(vec![upload(0, "a.txt"), upload(1, "b.txt")]);

    press(&mut test, KeyCode::Down);

    assert_eq!(test.app.sync_view.cursor(), 1);
}

#[test]
fn enter_runs_the_ticked_rows_and_returns_to_the_files() {
    let mut test = with_plan(vec![upload(0, "a.txt"), upload(1, "b.txt")]);
    press(&mut test, KeyCode::Char(' '));

    press(&mut test, KeyCode::Enter);

    assert_eq!(test.sent(), vec![Command::RunSync { sync_id: 11, choices: vec![(1, SyncAction::Upload)] }]);
    assert_eq!(test.app.screen, Screen::Files);
    assert!(!test.app.sync_view.has_plan());
}

#[test]
fn enter_with_nothing_ticked_stays_put_and_says_so() {
    let mut test = with_plan(vec![upload(0, "a.txt")]);
    press(&mut test, KeyCode::Char('n'));

    press(&mut test, KeyCode::Enter);

    assert!(test.sent().is_empty());
    assert_eq!(test.app.screen, Screen::Sync);
    assert!(test.app.notifications.current().is_some_and(|note| note.message.contains("Nothing is ticked")));
}

#[test]
fn escape_cancels_the_plan_and_tells_the_core() {
    let mut test = with_plan(vec![upload(0, "a.txt")]);

    press(&mut test, KeyCode::Esc);

    assert_eq!(test.sent(), vec![Command::CancelSync { sync_id: 11 }]);
    assert_eq!(test.app.screen, Screen::Files);
    assert!(!test.app.sync_view.has_plan());
}

#[test]
fn escape_clears_an_active_filter_before_it_cancels_anything() {
    let mut test = with_plan(vec![upload(0, "a.txt")]);
    press(&mut test, KeyCode::Char('/'));
    press(&mut test, KeyCode::Char('a'));
    press(&mut test, KeyCode::Enter);

    press(&mut test, KeyCode::Esc);

    assert_eq!(test.app.screen, Screen::Sync);
    assert!(test.sent().is_empty());
    assert_eq!(test.app.sync_view.matched(), 1);
}

#[test]
fn a_withdrawn_plan_closes_the_preview_and_says_so() {
    let mut test = with_plan(vec![upload(0, "a.txt")]);

    test.app.apply_core_event(Event::SyncWithdrawn { sync_ids: vec![11] });

    assert_eq!(test.app.screen, Screen::Files);
    assert!(!test.app.sync_view.has_plan());
    assert!(test.app.notifications.current().is_some_and(|note| note.message.contains("cancelled")));
}

#[test]
fn a_withdrawal_of_another_plan_changes_nothing() {
    let mut test = with_plan(vec![upload(0, "a.txt")]);

    test.app.apply_core_event(Event::SyncWithdrawn { sync_ids: vec![99] });

    assert_eq!(test.app.screen, Screen::Sync);
    assert!(test.app.sync_view.has_plan());
}

#[test]
fn the_preview_draws_inside_the_whole_app() {
    use ratatui::{Terminal, backend::TestBackend};

    let test = with_plan(vec![upload(0, "a.txt")]);
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();

    terminal.draw(|frame| test.app.render(frame)).unwrap();

    let screen: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    assert!(screen.contains("Sync with prod"), "{screen}");
    assert!(screen.contains("a.txt"), "{screen}");
}

#[test]
fn f_flips_the_focused_row_and_the_run_carries_the_new_direction() {
    let mut test = with_plan(vec![newer_on_target(0, "a.txt")]);
    assert!(test.app.sync_view.choices().is_empty());

    press(&mut test, KeyCode::Char('f'));
    press(&mut test, KeyCode::Enter);

    assert_eq!(test.sent(), vec![Command::RunSync { sync_id: 11, choices: vec![(0, SyncAction::Upload)] }]);
}

#[test]
fn a_newer_target_flipped_then_unticked_is_not_ticked_again_by_a() {
    let mut test = with_plan(vec![upload(0, "a.txt"), newer_on_target(1, "b.txt")]);
    press(&mut test, KeyCode::Down);
    press(&mut test, KeyCode::Char('f'));

    press(&mut test, KeyCode::Char('n'));
    press(&mut test, KeyCode::Char('a'));
    press(&mut test, KeyCode::Enter);

    assert_eq!(test.sent(), vec![Command::RunSync { sync_id: 11, choices: vec![(0, SyncAction::Upload)] }]);
}

#[test]
fn enter_with_a_filter_runs_only_the_ticked_rows_it_shows() {
    let mut test = with_plan(vec![upload(0, "a.txt"), upload(1, "b.txt")]);
    press(&mut test, KeyCode::Char('/'));
    press(&mut test, KeyCode::Char('b'));
    press(&mut test, KeyCode::Enter);

    press(&mut test, KeyCode::Enter);

    assert_eq!(test.sent(), vec![Command::RunSync { sync_id: 11, choices: vec![(1, SyncAction::Upload)] }]);
}

#[test]
fn enter_with_a_filter_that_shows_nothing_ticked_stays_put_even_if_hidden_rows_are_ticked() {
    let mut test = with_plan(vec![upload(0, "a.txt"), upload(1, "b.txt")]);
    press(&mut test, KeyCode::Char('/'));
    press(&mut test, KeyCode::Char('b'));
    press(&mut test, KeyCode::Enter);
    press(&mut test, KeyCode::Char('n'));

    press(&mut test, KeyCode::Enter);

    assert!(test.sent().is_empty());
    assert_eq!(test.app.screen, Screen::Sync);
    assert!(test.app.notifications.current().is_some_and(|note| note.message.contains("Nothing is ticked")));
}

#[test]
fn a_second_plan_while_the_preview_is_open_is_cancelled_and_the_first_keeps_its_ticks() {
    let mut test = with_plan(vec![upload(0, "a.txt"), upload(1, "b.txt")]);
    press(&mut test, KeyCode::Char(' '));

    test.app.apply_core_event(Event::SyncPlanReady(plan_with_id(12, vec![upload(0, "z.txt")])));

    assert_eq!(test.sent(), vec![Command::CancelSync { sync_id: 12 }]);
    assert_eq!(test.app.sync_view.sync_id(), Some(11));
    assert_eq!(test.app.sync_view.choices(), vec![(1, SyncAction::Upload)]);
    assert_eq!(test.app.screen, Screen::Sync);
    let note = test.app.notifications.current().unwrap();
    assert_eq!(note.severity, Severity::Warning);
    assert!(note.message.contains("already open"), "{}", note.message);
}

#[test]
fn a_second_plan_while_the_first_waits_elsewhere_is_cancelled_too() {
    let mut test = connected(true);
    test.app.screen = Screen::History;
    test.app.apply_core_event(Event::SyncPlanReady(plan_with(vec![upload(0, "a.txt")])));
    test.app.notifications.dismiss_current();

    test.app.apply_core_event(Event::SyncPlanReady(plan_with_id(12, vec![upload(0, "z.txt")])));

    assert_eq!(test.sent(), vec![Command::CancelSync { sync_id: 12 }]);
    assert_eq!(test.app.sync_view.sync_id(), Some(11));
    assert_eq!(test.app.screen, Screen::History);
    let note = test.app.notifications.current().unwrap();
    assert_eq!(note.severity, Severity::Warning);
    assert!(!note.message.contains("preview ready"), "{}", note.message);
}

#[test]
fn the_same_plan_arriving_twice_changes_and_cancels_nothing() {
    let mut test = with_plan(vec![upload(0, "a.txt")]);

    test.app.apply_core_event(Event::SyncPlanReady(plan_with(vec![upload(0, "a.txt")])));

    assert!(test.sent().is_empty());
    assert!(test.app.sync_view.has_plan());
}

#[test]
fn a_withdrawal_while_the_plan_waits_elsewhere_clears_it_and_leaves_the_screen_alone() {
    let mut test = connected(true);
    test.app.screen = Screen::History;
    test.app.apply_core_event(Event::SyncPlanReady(plan_with(vec![upload(0, "a.txt")])));
    test.app.notifications.dismiss_current();

    test.app.apply_core_event(Event::SyncWithdrawn { sync_ids: vec![11] });

    assert_eq!(test.app.screen, Screen::History);
    assert!(!test.app.sync_view.has_plan());
    assert!(test.app.notifications.current().is_some_and(|note| note.message.contains("cancelled")));
}

#[test]
fn the_key_does_nothing_away_from_the_files_even_with_a_waiting_plan() {
    let mut test = connected(true);
    test.app.screen = Screen::History;
    test.app.apply_core_event(Event::SyncPlanReady(plan_with(vec![upload(0, "a.txt")])));

    open(&mut test);

    assert_eq!(test.app.screen, Screen::History);
    assert!(test.app.dialog.is_none());
    assert!(test.sent().is_empty());
}

#[test]
fn escape_dismisses_a_showing_error_before_it_cancels_the_plan() {
    let mut test = with_plan(vec![upload(0, "a.txt")]);
    test.app.notifications.push(Severity::Error, "boom");

    press(&mut test, KeyCode::Esc);

    assert_eq!(test.app.screen, Screen::Sync);
    assert!(test.app.sync_view.has_plan());
    assert!(test.sent().is_empty());
    assert!(test.app.notifications.current().is_none());

    press(&mut test, KeyCode::Esc);

    assert_eq!(test.sent(), vec![Command::CancelSync { sync_id: 11 }]);
    assert_eq!(test.app.screen, Screen::Files);
}

#[test]
fn a_plan_does_not_take_over_the_screen_beneath_the_help_overlay() {
    let mut test = connected(true);
    test.app.help_visible = true;

    test.app.apply_core_event(Event::SyncPlanReady(plan_with(vec![upload(0, "a.txt")])));

    assert_eq!(test.app.screen, Screen::Files);
    assert!(test.app.sync_view.has_plan());
    assert!(test.app.notifications.current().is_some_and(|note| note.message.contains("Ctrl+U")));
}

#[test]
fn a_plan_does_not_take_over_the_screen_while_a_filter_is_being_typed() {
    let mut test = connected(true);
    press(&mut test, KeyCode::Char('/'));
    press(&mut test, KeyCode::Char('x'));

    test.app.apply_core_event(Event::SyncPlanReady(plan_with(vec![upload(0, "a.txt")])));
    press(&mut test, KeyCode::Char('y'));

    assert_eq!(test.app.screen, Screen::Files);
    assert!(test.app.sync_view.has_plan());
    assert!(test.app.notifications.current().is_some_and(|note| note.message.contains("Ctrl+U")));
}
