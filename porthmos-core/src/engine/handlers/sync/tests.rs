use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use porthmos_vfs::testing::FakeFs;

use super::*;
use crate::{
    engine::{
        Command, Event,
        testing::{TestEngine, test_engine},
    },
    sync::{SyncBy, SyncDirection, SyncReason},
    transfer::rows::RowKind,
};

const REMOTE_ROOT: &str = "/srv";

fn options(direction: SyncDirection, by: SyncBy) -> SyncOptions {
    SyncOptions { direction, by, subfolders: true }
}

fn connect(t: &mut TestEngine, name: &str) -> (u64, FakeFs) {
    let (session, remote) = t.add_session(name);
    remote.dir(REMOTE_ROOT);
    (session, remote)
}

fn connect_with(t: &mut TestEngine, name: &str, remote: FakeFs) -> u64 {
    remote.dir(REMOTE_ROOT);
    t.add_session_with(name, Arc::new(remote))
}

fn work_dir(t: &TestEngine) -> PathBuf {
    let dir = t.dir.path().join("work");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn set_local_time(path: &Path, seconds: u64) {
    std::fs::File::open(path).unwrap().set_modified(UNIX_EPOCH + Duration::from_secs(seconds)).unwrap();
}

fn write_local(t: &TestEngine, name: &str, data: &[u8], seconds: u64) -> PathBuf {
    let path = work_dir(t).join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, data).unwrap();
    set_local_time(&path, seconds);
    path
}

fn plan_of(events: Vec<Event>) -> Option<Arc<SyncPlan>> {
    events.into_iter().find_map(|event| match event {
        Event::SyncPlanReady(plan) => Some(plan),
        _ => None,
    })
}

fn start(t: &mut TestEngine, session: u64, options: SyncOptions) {
    let local_dir = work_dir(t);
    t.engine.handle_command(Command::StartSync { session, local_dir, remote_dir: PathBuf::from(REMOTE_ROOT), options });
}

async fn scan(t: &mut TestEngine, session: u64, options: SyncOptions) -> Arc<SyncPlan> {
    start(t, session, options);
    t.run_internal().await;
    plan_of(t.drain()).expect("a sync plan")
}

#[tokio::test]
async fn a_scan_ends_in_a_plan_with_the_differences() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/theirs.txt", b"x", Some(10));
    write_local(&t, "mine.txt", b"hello", 1_000);

    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;

    assert_eq!(plan.session, session);
    assert_eq!(plan.remote_root, PathBuf::from(REMOTE_ROOT));
    assert_eq!(plan.local_root, t.dir.path().join("work"));
    let items: Vec<(String, SyncReason)> =
        plan.items.iter().map(|item| (item.path.display().to_string(), item.reason)).collect();
    assert_eq!(items, vec![("mine.txt".to_string(), SyncReason::OnlyLocal)]);
    assert!(t.engine.sync.plans.contains_key(&plan.sync_id));
}

#[tokio::test]
async fn folders_already_in_sync_produce_a_notice_and_no_plan() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/same.txt", b"hello", Some(1_000));
    write_local(&t, "same.txt", b"hello", 1_000);
    start(&mut t, session, options(SyncDirection::Both, SyncBy::Time));

    t.run_internal().await;

    let events = t.drain();
    assert!(plan_of(events.clone()).is_none());
    assert!(
        events.iter().any(|event| matches!(event, Event::Notice { message, .. } if message == "Folders are in sync"))
    );
    assert!(t.engine.sync.plans.is_empty());
}

#[tokio::test]
async fn the_scan_shows_as_a_scanning_row_until_it_finishes() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));

    assert_eq!(t.engine.planning.len(), 1);
    assert!(t.engine.sync.scanning.contains(&t.engine.planning[0].batch_id));

    t.run_internal().await;

    assert!(t.engine.planning.is_empty());
    assert!(t.engine.sync.scanning.is_empty());
}

#[tokio::test]
async fn the_tolerance_comes_from_the_coarser_filesystem() {
    let mut t = test_engine();
    let remote = FakeFs::new().with_time_resolution(60);
    remote.file("/srv/a.txt", b"hello", Some(1_000));
    let session = connect_with(&mut t, "coarse", remote);
    write_local(&t, "a.txt", b"hello", 1_030);
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));

    t.run_internal().await;

    assert!(plan_of(t.drain()).is_none());
}

#[tokio::test]
async fn both_is_refused_when_the_remote_cannot_keep_times() {
    let mut t = test_engine();
    let session = connect_with(&mut t, "flat", FakeFs::new().with_time_support(false));
    t.drain();

    start(&mut t, session, options(SyncDirection::Both, SyncBy::Time));

    assert!(t.engine.planning.is_empty());
    let notices = t.notices();
    assert!(
        notices.iter().any(|(severity, message)| *severity == crate::Severity::Warning && message.contains("flat")),
        "{notices:?}"
    );
}

#[tokio::test]
async fn one_way_works_on_a_connection_that_cannot_keep_times() {
    let mut t = test_engine();
    let session = connect_with(&mut t, "flat", FakeFs::new().with_time_support(false));
    write_local(&t, "a.txt", b"x", 5);

    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;

    assert_eq!(plan.items.len(), 1);
}

#[tokio::test]
async fn both_cannot_compare_by_size() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    t.drain();

    start(&mut t, session, options(SyncDirection::Both, SyncBy::Size));

    assert!(t.engine.planning.is_empty());
    assert!(t.notices().iter().any(|(_, message)| message.contains("size")));
}

#[tokio::test]
async fn an_unknown_session_is_a_warning() {
    let mut t = test_engine();
    t.drain();

    start(&mut t, 999, options(SyncDirection::LocalToRemote, SyncBy::Time));

    assert!(t.engine.planning.is_empty());
    assert!(
        t.notices().iter().any(|(severity, message)| *severity == crate::Severity::Warning
            && message == "Sync failed: session disconnected")
    );
}

#[tokio::test]
async fn an_unreadable_folder_fails_the_scan_with_an_error_and_no_plan() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    t.drain();
    let local_dir = work_dir(&t);
    t.engine.handle_command(Command::StartSync {
        session,
        local_dir,
        remote_dir: PathBuf::from("/does/not/exist"),
        options: options(SyncDirection::LocalToRemote, SyncBy::Time),
    });

    t.run_internal().await;

    let events = t.drain();
    assert!(plan_of(events.clone()).is_none());
    assert!(events.iter().any(|event| matches!(event, Event::Notice { severity: crate::Severity::Error, message } if message.contains("Sync failed"))));
    assert!(t.engine.planning.is_empty() && t.engine.sync.scanning.is_empty());
}

#[tokio::test]
async fn cancelling_a_scan_ends_it_without_a_plan() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));
    let sync_id = t.engine.planning[0].batch_id;
    t.drain();

    t.engine.handle_command(Command::CancelSync { sync_id });
    t.run_internal().await;

    let events = t.drain();
    assert!(plan_of(events.clone()).is_none());
    assert!(events.iter().any(|event| matches!(event, Event::Notice { message, .. } if message == "Sync cancelled")));
    assert!(t.engine.planning.is_empty());
    assert!(t.engine.sync.scanning.is_empty());
}

#[tokio::test]
async fn cancel_sync_ignores_a_batch_that_is_not_a_sync_scan() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    let batch = t.engine.transfers.start_batch("copy".to_string());
    t.engine.planning.push(PlanningScan {
        batch_id: batch,
        session_id: session,
        direction: Direction::Upload,
        display_name: "copy".to_string(),
    });

    t.engine.handle_command(Command::CancelSync { sync_id: batch });

    assert!(!t.engine.tasks.take_cancelled(crate::tasks::Scope::Planning(batch)));
}

#[tokio::test]
async fn cancelling_a_stored_plan_drops_it() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;

    t.engine.handle_command(Command::CancelSync { sync_id: plan.sync_id });

    assert!(t.engine.sync.plans.is_empty());
    assert!(!t.drain().iter().any(|event| matches!(event, Event::SyncWithdrawn { .. })));
}

#[tokio::test]
async fn disconnecting_withdraws_the_sessions_plans_and_only_those() {
    let mut t = test_engine();
    let (first, _a) = connect(&mut t, "one");
    let (second, _b) = connect(&mut t, "two");
    write_local(&t, "a.txt", b"x", 5);
    let plan_one = scan(&mut t, first, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    let plan_two = scan(&mut t, second, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;

    t.engine.disconnect(first);

    let withdrawn: Vec<Vec<u64>> = t
        .drain()
        .into_iter()
        .filter_map(|event| match event {
            Event::SyncWithdrawn { sync_ids } => Some(sync_ids),
            _ => None,
        })
        .collect();
    assert_eq!(withdrawn, vec![vec![plan_one.sync_id]]);
    assert!(t.engine.sync.plans.contains_key(&plan_two.sync_id));
}

#[tokio::test]
async fn cancelling_all_transfers_withdraws_every_plan() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;

    t.engine.handle_command(Command::CancelAllTransfers);

    assert!(t.engine.sync.plans.is_empty());
    assert!(
        t.drain()
            .iter()
            .any(|event| matches!(event, Event::SyncWithdrawn { sync_ids } if sync_ids == &vec![plan.sync_id]))
    );
}

#[tokio::test]
async fn disconnecting_during_a_scan_cancels_it_quietly() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));
    t.engine.disconnect(session);
    t.drain();

    t.run_internal().await;

    let events = t.drain();
    assert!(plan_of(events.clone()).is_none());
    assert!(!events.iter().any(|event| matches!(event, Event::Notice { message, .. } if message == "Sync cancelled")));
    assert!(t.engine.planning.is_empty() && t.engine.sync.scanning.is_empty());
}

#[tokio::test]
async fn symlinks_are_skipped_with_a_warning() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    std::os::unix::fs::symlink(work_dir(&t).join("a.txt"), work_dir(&t).join("link")).unwrap();
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));

    t.run_internal().await;

    let events = t.drain();
    assert!(events.iter().any(|event| matches!(event, Event::Notice { severity: crate::Severity::Warning, message } if message.starts_with("Skipped 1 symlink"))));
    assert_eq!(plan_of(events).unwrap().skipped_symlinks, 1);
}

fn cancelled_notice(events: &[Event]) -> bool {
    events.iter().any(|event| matches!(event, Event::Notice { message, .. } if message == "Sync cancelled"))
}

async fn finished_scan(t: &mut TestEngine) -> Internal {
    tokio::time::timeout(Duration::from_secs(2), t.internal.recv())
        .await
        .expect("an internal event within two seconds")
        .expect("the internal channel is open")
}

fn assert_ended_cancelled(t: &mut TestEngine) {
    let events = t.drain();
    assert!(plan_of(events.clone()).is_none());
    assert!(cancelled_notice(&events), "{events:?}");
    assert!(t.engine.sync.plans.is_empty());
    assert!(t.engine.planning.is_empty() && t.engine.sync.scanning.is_empty());
}

#[tokio::test]
async fn a_panicking_scan_is_a_sync_failure_not_a_failed_copy() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));
    let sync_id = t.engine.planning[0].batch_id;
    t.drain();

    t.engine.handle_internal(Internal::TaskPanicked {
        name: "sync-scan",
        scope: Scope::Planning(sync_id),
        message: "boom".to_string(),
    });

    assert!(t.engine.planning.is_empty() && t.engine.sync.scanning.is_empty());
    assert!(t.engine.history.store.entries().is_empty());
    assert!(t.engine.transfers.batch_label(sync_id).is_none());
    let notices = t.notices();
    assert!(
        notices.iter().any(|(severity, message)| *severity == crate::Severity::Error
            && message == "A background task failed: sync-scan"),
        "{notices:?}"
    );
}

#[tokio::test]
async fn a_plan_that_arrives_after_its_session_disconnected_is_dropped_with_an_error() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));
    let done = finished_scan(&mut t).await;
    t.engine.sessions.remove(&session);
    t.drain();

    t.engine.handle_internal(done);

    let events = t.drain();
    assert!(plan_of(events.clone()).is_none());
    assert!(events.iter().any(|event| matches!(event, Event::Notice { severity: crate::Severity::Error, message } if message == "Sync failed: session disconnected")));
    assert!(t.engine.sync.plans.is_empty());
    assert!(t.engine.planning.is_empty() && t.engine.sync.scanning.is_empty());
}

#[tokio::test]
async fn a_cancel_that_arrives_after_the_scan_finished_still_cancels_it() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));
    let sync_id = t.engine.planning[0].batch_id;
    let done = finished_scan(&mut t).await;
    t.drain();

    t.engine.handle_command(Command::CancelSync { sync_id });
    t.engine.handle_internal(done);

    assert_ended_cancelled(&mut t);
}

#[tokio::test]
async fn cancelling_all_transfers_after_the_scan_finished_still_cancels_it() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));
    let done = finished_scan(&mut t).await;
    t.drain();

    t.engine.handle_command(Command::CancelAllTransfers);
    t.engine.handle_internal(done);

    assert_ended_cancelled(&mut t);
}

#[tokio::test]
async fn a_disconnect_after_the_scan_finished_cancels_it_quietly() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));
    let done = finished_scan(&mut t).await;
    t.engine.disconnect(session);
    t.drain();

    t.engine.handle_internal(done);

    let events = t.drain();
    assert!(plan_of(events.clone()).is_none());
    assert!(!cancelled_notice(&events));
    assert!(!events.iter().any(|event| matches!(event, Event::Notice { severity: crate::Severity::Error, .. })));
    assert!(t.engine.planning.is_empty() && t.engine.sync.scanning.is_empty());
}

#[tokio::test]
async fn a_running_scan_is_cancelled_from_its_row() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));
    let sync_id = t.engine.planning[0].batch_id;
    t.drain();

    t.engine.handle_command(Command::CancelRow { kind: RowKind::Scan(sync_id) });
    t.run_internal().await;

    assert_ended_cancelled(&mut t);
}

#[tokio::test]
async fn a_running_scan_is_cancelled_by_cancel_all() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));
    t.drain();

    t.engine.handle_command(Command::CancelAllTransfers);
    t.run_internal().await;

    assert_ended_cancelled(&mut t);
}

#[tokio::test]
async fn shutting_down_during_a_scan_writes_no_history_entry() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));

    t.engine.record_interrupted();

    assert!(t.engine.history.store.entries().is_empty());
}

#[tokio::test]
async fn a_copy_scan_is_still_recorded_as_interrupted_at_shutdown() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    let batch = t.engine.transfers.start_batch("copy".to_string());
    t.engine.planning.push(PlanningScan {
        batch_id: batch,
        session_id: session,
        direction: Direction::Upload,
        display_name: "copy".to_string(),
    });

    t.engine.record_interrupted();

    assert_eq!(t.engine.history.store.entries().len(), 1);
}

#[tokio::test]
async fn withdrawing_nothing_emits_nothing() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    t.drain();

    t.engine.disconnect(session);
    t.engine.handle_command(Command::CancelAllTransfers);

    assert!(!t.drain().iter().any(|event| matches!(event, Event::SyncWithdrawn { .. })));
}

#[tokio::test]
async fn several_symlinks_are_counted_in_the_plural() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"x", 5);
    std::os::unix::fs::symlink(work_dir(&t).join("a.txt"), work_dir(&t).join("link1")).unwrap();
    std::os::unix::fs::symlink(work_dir(&t).join("a.txt"), work_dir(&t).join("link2")).unwrap();
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));

    t.run_internal().await;

    let events = t.drain();
    assert!(events.iter().any(|event| matches!(event, Event::Notice { severity: crate::Severity::Warning, message } if message == "Skipped 2 symlinks")), "{events:?}");
}

fn time_less_local_with(name: &str, data: &[u8], seconds: u64) -> FakeFs {
    let local = FakeFs::new().with_time_support(false);
    local.dir("/work");
    local.file(Path::new("/work").join(name), data, Some(seconds));
    local
}

#[tokio::test]
async fn a_same_size_target_stamped_newer_is_not_listed_when_the_remote_target_cannot_keep_times() {
    let mut t = test_engine();
    let remote = FakeFs::new().with_time_support(false);
    remote.file("/srv/a.txt", b"x", Some(5_000));
    let session = connect_with(&mut t, "flat", remote);
    write_local(&t, "a.txt", b"x", 1_000);
    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));

    t.run_internal().await;

    assert!(plan_of(t.drain()).is_none());
}

#[tokio::test]
async fn a_same_size_target_stamped_newer_is_listed_when_the_remote_target_keeps_times() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/a.txt", b"x", Some(5_000));
    write_local(&t, "a.txt", b"x", 1_000);

    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;

    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].reason, SyncReason::TargetNewer);
}

#[tokio::test]
async fn a_same_size_target_stamped_newer_is_not_listed_when_the_local_target_cannot_keep_times() {
    let mut t = test_engine();
    t.engine.local_fs = Arc::new(time_less_local_with("a.txt", b"x", 5_000));
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/a.txt", b"x", Some(1_000));
    t.engine.handle_command(Command::StartSync {
        session,
        local_dir: PathBuf::from("/work"),
        remote_dir: PathBuf::from(REMOTE_ROOT),
        options: options(SyncDirection::RemoteToLocal, SyncBy::Time),
    });

    t.run_internal().await;

    assert!(plan_of(t.drain()).is_none());
}

#[tokio::test]
async fn a_same_size_target_stamped_newer_is_listed_when_the_local_target_keeps_times() {
    let mut t = test_engine();
    let local = FakeFs::new();
    local.dir("/work");
    local.file("/work/a.txt", b"x", Some(5_000));
    t.engine.local_fs = Arc::new(local);
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/a.txt", b"x", Some(1_000));
    t.engine.handle_command(Command::StartSync {
        session,
        local_dir: PathBuf::from("/work"),
        remote_dir: PathBuf::from(REMOTE_ROOT),
        options: options(SyncDirection::RemoteToLocal, SyncBy::Time),
    });

    t.run_internal().await;

    let plan = plan_of(t.drain()).expect("a sync plan");
    assert_eq!(plan.items[0].reason, SyncReason::TargetNewer);
}

fn ticked(plan: &SyncPlan) -> Vec<(u32, SyncAction)> {
    plan.items.iter().filter(|item| item.ticked).map(|item| (item.id, item.action)).collect()
}

async fn finish_transfers(t: &mut TestEngine) {
    for _ in 0..200 {
        let busy = t.engine.transfers.active_count() > 0
            || t.engine.transfers.queued_count() > 0
            || !t.engine.planning.is_empty();
        if !busy {
            return;
        }
        t.run_internal().await;
    }
    panic!("the transfers did not finish");
}

fn refusal_warnings(t: &mut TestEngine) -> Vec<String> {
    t.notices()
        .into_iter()
        .filter(|(severity, _)| *severity == crate::Severity::Warning)
        .map(|(_, message)| message)
        .collect()
}

async fn run(t: &mut TestEngine, plan: &SyncPlan, choices: Vec<(u32, SyncAction)>) {
    t.engine.handle_command(Command::RunSync { sync_id: plan.sync_id, choices });
    finish_transfers(t).await;
}

#[tokio::test]
async fn uploading_copies_the_files_and_keeps_their_times() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"hello", 1_000);
    write_local(&t, "deep/er/b.txt", b"world", 2_000);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;

    run(&mut t, &plan, ticked(&plan)).await;

    assert_eq!(remote.contents("/srv/a.txt").unwrap(), b"hello");
    assert_eq!(remote.contents("/srv/deep/er/b.txt").unwrap(), b"world");
    assert_eq!(remote.modified_of("/srv/a.txt"), Some(1_000));
    assert_eq!(remote.modified_of("/srv/deep/er/b.txt"), Some(2_000));
}

#[tokio::test]
async fn downloading_copies_the_files_and_keeps_their_times() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/sub/c.txt", b"remote data", Some(5_000));
    let plan = scan(&mut t, session, options(SyncDirection::RemoteToLocal, SyncBy::Time)).await;

    run(&mut t, &plan, ticked(&plan)).await;

    let copy = t.dir.path().join("work/sub/c.txt");
    assert_eq!(std::fs::read(&copy).unwrap(), b"remote data");
    let modified = std::fs::metadata(&copy).unwrap().modified().unwrap();
    assert_eq!(modified, UNIX_EPOCH + Duration::from_secs(5_000));
}

#[tokio::test]
async fn both_copies_in_each_direction_as_two_batches() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    write_local(&t, "up.txt", b"up", 1_000);
    remote.file("/srv/down.txt", b"down", Some(2_000));
    let plan = scan(&mut t, session, options(SyncDirection::Both, SyncBy::Time)).await;

    t.engine.handle_command(Command::RunSync { sync_id: plan.sync_id, choices: ticked(&plan) });
    finish_transfers(&mut t).await;

    assert_eq!(remote.contents("/srv/up.txt").unwrap(), b"up");
    assert_eq!(std::fs::read(t.dir.path().join("work/down.txt")).unwrap(), b"down");
    assert_eq!(t.engine.snapshot().rows.len(), 2);
}

#[tokio::test]
async fn overwriting_an_older_target_raises_no_conflict_question() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/a.txt", b"old", Some(100));
    write_local(&t, "a.txt", b"brand new", 5_000);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    t.drain();

    run(&mut t, &plan, ticked(&plan)).await;

    assert!(!t.drain().iter().any(|event| matches!(event, Event::ConflictsFound { .. })));
    assert_eq!(remote.contents("/srv/a.txt").unwrap(), b"brand new");
}

#[tokio::test]
async fn items_that_are_not_chosen_are_not_copied() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    write_local(&t, "keep.txt", b"1", 10);
    write_local(&t, "skip.txt", b"2", 10);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    let keep = plan.items.iter().find(|item| item.path == Path::new("keep.txt")).unwrap();

    run(&mut t, &plan, vec![(keep.id, keep.action)]).await;

    assert!(remote.exists("/srv/keep.txt"));
    assert!(!remote.exists("/srv/skip.txt"));
}

#[tokio::test]
async fn a_newer_target_is_only_overwritten_when_the_user_flips_it() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/a.txt", b"remote is newer", Some(9_000));
    write_local(&t, "a.txt", b"local is older", 1_000);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    let item = plan.items[0].clone();
    assert_eq!((item.reason, item.ticked, item.action), (SyncReason::TargetNewer, false, SyncAction::Skip));

    run(&mut t, &plan, ticked(&plan)).await;
    assert_eq!(remote.contents("/srv/a.txt").unwrap(), b"remote is newer");

    let again = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    run(&mut t, &again, vec![(again.items[0].id, SyncAction::Upload)]).await;
    assert_eq!(remote.contents("/srv/a.txt").unwrap(), b"local is older");
}

#[tokio::test]
async fn choices_that_do_not_apply_are_refused_with_one_warning() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"1", 10);
    write_local(&t, "b.txt", b"2", 10);
    write_local(&t, "c.txt", b"3", 10);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    t.drain();

    run(
        &mut t,
        &plan,
        vec![
            (0, SyncAction::Upload),
            (1, SyncAction::Download),
            (99, SyncAction::Upload),
            (0, SyncAction::Upload),
            (2, SyncAction::Skip),
        ],
    )
    .await;

    assert!(remote.exists("/srv/a.txt"));
    assert!(!remote.exists("/srv/b.txt"));
    assert!(!remote.exists("/srv/c.txt"));
    assert_eq!(refusal_warnings(&mut t), vec!["Ignored 3 sync choices that don't apply".to_string()]);
}

#[tokio::test]
async fn a_flip_on_a_row_that_cannot_be_flipped_is_refused() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    write_local(&t, "dir/inner.txt", b"x", 10);
    remote.file("/srv/dir", b"a file where a folder is", Some(10));
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    let item = plan.items[0].clone();
    assert_eq!(item.reason, SyncReason::KindMismatch);

    t.drain();

    run(&mut t, &plan, vec![(item.id, SyncAction::Upload)]).await;

    assert_eq!(remote.contents("/srv/dir").unwrap(), b"a file where a folder is");
    assert_eq!(refusal_warnings(&mut t), vec!["Ignored 1 sync choice that doesn't apply".to_string()]);
}

#[tokio::test]
async fn an_unknown_or_used_plan_is_a_warning_and_changes_nothing() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"1", 10);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    run(&mut t, &plan, ticked(&plan)).await;
    t.drain();

    t.engine.handle_command(Command::RunSync { sync_id: plan.sync_id, choices: ticked(&plan) });
    t.engine.handle_command(Command::RunSync { sync_id: 424_242, choices: Vec::new() });

    let warnings: Vec<String> = t.notices().into_iter().map(|(_, message)| message).collect();
    assert_eq!(warnings, vec!["That sync plan is no longer available".to_string(); 2]);
    assert!(t.engine.planning.is_empty());
}

#[tokio::test]
async fn running_with_nothing_chosen_says_so() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"1", 10);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    t.drain();

    t.engine.handle_command(Command::RunSync { sync_id: plan.sync_id, choices: Vec::new() });

    assert!(t.notices().iter().any(|(_, message)| message == "Nothing to sync"));
    assert!(t.engine.planning.is_empty() && t.engine.sync.plans.is_empty());
}

#[tokio::test]
async fn a_session_that_vanished_before_running_is_an_error() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"1", 10);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    t.engine.sessions.remove(&session);
    t.drain();

    t.engine.handle_command(Command::RunSync { sync_id: plan.sync_id, choices: ticked(&plan) });

    assert!(
        t.notices()
            .iter()
            .any(|(severity, message)| *severity == crate::Severity::Error && message.contains("disconnected"))
    );
    assert!(t.engine.planning.is_empty() && t.engine.sync.scanning.is_empty() && t.engine.sync.plans.is_empty());
    assert!(t.engine.transfers.active_count() == 0 && t.engine.transfers.queued_count() == 0);
    assert!(t.engine.snapshot().rows.is_empty());
}

#[tokio::test]
async fn a_folder_that_cannot_be_created_fails_only_its_own_direction() {
    use std::os::unix::fs::PermissionsExt;

    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/sub/down.txt", b"d", Some(50));
    write_local(&t, "up.txt", b"u", 60);
    let locked = t.dir.path().join("work");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    if std::fs::File::create(locked.join("probe")).is_ok() {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let plan = scan(&mut t, session, options(SyncDirection::Both, SyncBy::Time)).await;
    t.drain();

    run(&mut t, &plan, ticked(&plan)).await;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        t.notices()
            .iter()
            .any(|(severity, message)| *severity == crate::Severity::Error && message.contains("Sync failed"))
    );
    assert_eq!(remote.contents("/srv/up.txt").unwrap(), b"u");
    assert!(!t.dir.path().join("work/sub/down.txt").exists());
}

#[tokio::test]
async fn a_one_way_upload_to_a_connection_that_cannot_keep_times_also_settles() {
    let mut t = test_engine();
    let session = connect_with(&mut t, "flat", FakeFs::new().with_time_support(false));
    write_local(&t, "a.txt", b"hello", 1_000);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    run(&mut t, &plan, ticked(&plan)).await;
    t.drain();

    start(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time));
    t.run_internal().await;

    let events = t.drain();
    assert!(plan_of(events.clone()).is_none(), "{events:?}");
}

#[tokio::test]
async fn a_second_sync_right_after_the_first_finds_nothing_to_do() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    write_local(&t, "a.txt", b"hello", 1_000);
    write_local(&t, "sub/b.txt", b"world", 2_000);
    remote.file("/srv/c.txt", b"remote only", Some(3_000));
    let plan = scan(&mut t, session, options(SyncDirection::Both, SyncBy::Time)).await;
    run(&mut t, &plan, ticked(&plan)).await;
    t.drain();

    start(&mut t, session, options(SyncDirection::Both, SyncBy::Time));
    t.run_internal().await;

    let events = t.drain();
    assert!(plan_of(events.clone()).is_none(), "{events:?}");
    assert!(
        events.iter().any(|event| matches!(event, Event::Notice { message, .. } if message == "Folders are in sync"))
    );
}

fn planning_batch(t: &TestEngine, direction: Direction) -> u64 {
    t.engine.planning.iter().find(|scan| scan.direction == direction).expect("a planning row").batch_id
}

#[tokio::test]
async fn a_crafted_download_of_a_local_newer_file_is_refused_in_both() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/a.txt", b"older remote", Some(100));
    write_local(&t, "a.txt", b"newer local file", 9_000);
    let plan = scan(&mut t, session, options(SyncDirection::Both, SyncBy::Time)).await;
    let item = plan.items[0].clone();
    assert_eq!((item.reason, item.action, item.flippable), (SyncReason::LocalNewer, SyncAction::Upload, false));
    assert!(item.allows(SyncAction::Download, SyncDirection::Both));
    t.drain();

    run(&mut t, &plan, vec![(item.id, SyncAction::Download)]).await;

    assert_eq!(std::fs::read(t.dir.path().join("work/a.txt")).unwrap(), b"newer local file");
    assert_eq!(remote.contents("/srv/a.txt").unwrap(), b"older remote");
    assert_eq!(refusal_warnings(&mut t), vec!["Ignored 1 sync choice that doesn't apply".to_string()]);
}

#[tokio::test]
async fn folder_creation_is_a_sync_row_until_it_is_ready() {
    let mut t = test_engine();
    let (session, _remote) = connect(&mut t, "prod");
    write_local(&t, "deep/a.txt", b"1", 10);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;

    t.engine.handle_command(Command::RunSync { sync_id: plan.sync_id, choices: ticked(&plan) });

    let batch = planning_batch(&t, Direction::Upload);
    assert!(t.engine.sync.scanning.contains(&batch));
    assert!(t.engine.is_sync_scan(RowKind::Scan(batch)));
    finish_transfers(&mut t).await;
    assert!(t.engine.sync.scanning.is_empty());
}

#[tokio::test]
async fn a_panicking_folder_creation_fails_only_its_own_direction_and_writes_no_history() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    write_local(&t, "up.txt", b"u", 10);
    remote.file("/srv/down.txt", b"d", Some(20));
    let plan = scan(&mut t, session, options(SyncDirection::Both, SyncBy::Time)).await;
    t.drain();
    t.engine.handle_command(Command::RunSync { sync_id: plan.sync_id, choices: ticked(&plan) });
    let upload = planning_batch(&t, Direction::Upload);

    t.engine.handle_internal(Internal::TaskPanicked {
        name: "sync-folders",
        scope: Scope::Planning(upload),
        message: "boom".to_string(),
    });

    assert_eq!(t.engine.planning.len(), 1);
    assert!(!t.engine.sync.scanning.contains(&upload));
    assert!(t.engine.history.store.entries().is_empty());
    assert!(t.engine.transfers.batch_label(upload).is_none());
    let notices = t.notices();
    assert!(
        notices.iter().any(|(severity, message)| *severity == crate::Severity::Error
            && message == "A background task failed: sync-folders"),
        "{notices:?}"
    );

    finish_transfers(&mut t).await;

    assert_eq!(std::fs::read(t.dir.path().join("work/down.txt")).unwrap(), b"d");
    assert!(!remote.exists("/srv/up.txt"));
    assert!(t.engine.sync.scanning.is_empty() && t.engine.planning.is_empty());
}

#[tokio::test]
async fn shutting_down_during_folder_creation_writes_no_history_for_that_row() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    write_local(&t, "up.txt", b"u", 10);
    remote.file("/srv/down.txt", b"d", Some(20));
    let plan = scan(&mut t, session, options(SyncDirection::Both, SyncBy::Time)).await;
    t.engine.handle_command(Command::RunSync { sync_id: plan.sync_id, choices: ticked(&plan) });
    assert_eq!(t.engine.planning.len(), 2);

    t.engine.record_interrupted();

    assert!(t.engine.history.store.entries().is_empty());
}

#[tokio::test]
async fn a_cancel_during_folder_creation_copies_nothing_and_writes_no_history() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    write_local(&t, "deep/up.txt", b"u", 10);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    t.engine.handle_command(Command::RunSync { sync_id: plan.sync_id, choices: ticked(&plan) });
    t.drain();

    t.engine.handle_command(Command::CancelAllTransfers);
    finish_transfers(&mut t).await;

    assert!(!remote.exists("/srv/deep/up.txt"));
    assert!(cancelled_notice(&t.drain()));
    assert!(t.engine.history.store.entries().is_empty());
    assert!(t.engine.sync.scanning.is_empty() && t.engine.planning.is_empty());
}

#[tokio::test]
async fn folders_that_finish_after_their_batch_was_dropped_copy_nothing() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    write_local(&t, "up.txt", b"u", 10);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    t.engine.handle_command(Command::RunSync { sync_id: plan.sync_id, choices: ticked(&plan) });
    let batch = planning_batch(&t, Direction::Upload);
    let done = finished_scan(&mut t).await;
    t.engine.sync_failed(batch, "Sync failed".to_string());

    t.engine.handle_internal(done);

    assert!(t.engine.transfers.active_count() == 0 && t.engine.transfers.queued_count() == 0);
    assert!(!remote.exists("/srv/up.txt"));
}

#[tokio::test]
async fn a_cancel_that_arrives_after_the_folders_were_made_still_cancels_the_batch() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    write_local(&t, "up.txt", b"u", 10);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    t.engine.handle_command(Command::RunSync { sync_id: plan.sync_id, choices: ticked(&plan) });
    let done = finished_scan(&mut t).await;
    t.drain();

    t.engine.handle_command(Command::CancelAllTransfers);
    t.engine.handle_internal(done);

    assert!(cancelled_notice(&t.drain()));
    assert!(!remote.exists("/srv/up.txt"));
    assert!(t.engine.transfers.active_count() == 0 && t.engine.transfers.queued_count() == 0);
    assert!(t.engine.history.store.entries().is_empty());
    assert!(t.engine.sync.scanning.is_empty() && t.engine.planning.is_empty());
}

#[tokio::test]
async fn a_crafted_download_in_an_upload_only_sync_is_refused_even_on_a_flippable_row() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/a.txt", b"remote is newer", Some(9_000));
    let local = write_local(&t, "a.txt", b"local is older", 1_000);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    let item = plan.items[0].clone();
    assert_eq!((item.reason, item.flippable), (SyncReason::TargetNewer, true));
    assert!(!item.allows(SyncAction::Download, SyncDirection::LocalToRemote));

    run(&mut t, &plan, vec![(item.id, SyncAction::Download)]).await;

    assert_eq!(std::fs::read(local).unwrap(), b"local is older");
    assert_eq!(remote.contents("/srv/a.txt").unwrap(), b"remote is newer");
}

fn assert_symlink_in_the_way_is_listed_but_untickable(plan: &SyncPlan) {
    assert_eq!(plan.items.len(), 1, "{:?}", plan.items);
    let item = &plan.items[0];
    assert_eq!(
        (item.reason, item.ticked, item.flippable, item.action),
        (SyncReason::KindMismatch, false, false, SyncAction::Skip)
    );
}

fn nothing_was_queued(t: &TestEngine) -> bool {
    t.engine.transfers.active_count() == 0
        && t.engine.transfers.queued_count() == 0
        && t.engine.planning.is_empty()
        && t.engine.sync.scanning.is_empty()
}

#[tokio::test]
async fn an_upload_never_goes_through_a_symlink_on_the_remote_at_the_path_of_a_file() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/elsewhere/precious.txt", b"precious", Some(5));
    remote.symlink("/srv/a.txt", "/elsewhere/precious.txt");
    write_local(&t, "a.txt", b"local data", 9_000);
    let plan = scan(&mut t, session, options(SyncDirection::LocalToRemote, SyncBy::Time)).await;
    assert_symlink_in_the_way_is_listed_but_untickable(&plan);
    let id = plan.items[0].id;

    run(&mut t, &plan, vec![(id, SyncAction::Upload), (id, SyncAction::Download)]).await;

    assert!(nothing_was_queued(&t));
    assert_eq!(remote.contents("/srv/a.txt"), None);
    assert!(remote.exists("/srv/a.txt"));
    assert_eq!(remote.contents("/elsewhere/precious.txt").unwrap(), b"precious");
    assert_eq!(remote.modified_of("/elsewhere/precious.txt"), Some(5));
}

#[tokio::test]
async fn an_upload_never_goes_through_a_symlinked_remote_folder() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/elsewhere/keep.txt", b"keep", Some(5));
    remote.symlink("/srv/dir", "/elsewhere");
    write_local(&t, "dir/inner.txt", b"1", 10);
    write_local(&t, "dir/deeper/x.txt", b"2", 10);
    let plan = scan(&mut t, session, options(SyncDirection::Both, SyncBy::Time)).await;
    assert_symlink_in_the_way_is_listed_but_untickable(&plan);
    let id = plan.items[0].id;

    run(&mut t, &plan, vec![(id, SyncAction::Upload), (id, SyncAction::Download)]).await;

    assert!(nothing_was_queued(&t));
    for created in ["/elsewhere/inner.txt", "/elsewhere/deeper", "/srv/dir/inner.txt", "/srv/dir/deeper"] {
        assert!(!remote.exists(created), "{created} must not exist");
    }
    assert!(remote.exists("/srv/dir"));
    assert_eq!(remote.contents("/elsewhere/keep.txt").unwrap(), b"keep");
}

#[tokio::test]
async fn a_download_never_replaces_a_local_symlink_at_the_path_of_a_remote_file() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/a.txt", b"remote data", Some(9_000));
    let real = write_local(&t, "real.txt", b"precious", 5);
    let link = work_dir(&t).join("a.txt");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let plan = scan(&mut t, session, options(SyncDirection::RemoteToLocal, SyncBy::Time)).await;
    let mismatch: Vec<_> = plan.items.iter().filter(|item| item.path == Path::new("a.txt")).collect();
    assert_eq!(mismatch.len(), 1);
    assert_eq!(
        (mismatch[0].reason, mismatch[0].ticked, mismatch[0].flippable),
        (SyncReason::KindMismatch, false, false)
    );
    let choices: Vec<(u32, SyncAction)> =
        plan.items.iter().flat_map(|item| [(item.id, SyncAction::Download), (item.id, SyncAction::Upload)]).collect();

    run(&mut t, &plan, choices).await;

    assert!(nothing_was_queued(&t));
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read(&real).unwrap(), b"precious");
    assert_eq!(remote.contents("/srv/a.txt").unwrap(), b"remote data");
}

#[tokio::test]
async fn a_download_never_goes_through_a_local_symlinked_folder() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/d/one.txt", b"1", Some(9_000));
    remote.file("/srv/d/deeper/two.txt", b"2", Some(9_000));
    let outside = t.dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let link = work_dir(&t).join("d");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let plan = scan(&mut t, session, options(SyncDirection::Both, SyncBy::Time)).await;
    assert_symlink_in_the_way_is_listed_but_untickable(&plan);
    let id = plan.items[0].id;

    run(&mut t, &plan, vec![(id, SyncAction::Download), (id, SyncAction::Upload)]).await;

    assert!(nothing_was_queued(&t));
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert!(remote.exists("/srv/d/one.txt") && remote.exists("/srv/d/deeper/two.txt"));
}

#[tokio::test]
async fn without_subfolders_a_real_nested_remote_folder_produces_only_depth_one_items() {
    let mut t = test_engine();
    let (session, remote) = connect(&mut t, "prod");
    remote.file("/srv/top.txt", b"1", Some(9_000));
    remote.file("/srv/a/s/x.txt", b"2", Some(9_000));
    let outside = t.dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(work_dir(&t).join("a")).unwrap();
    std::os::unix::fs::symlink(&outside, work_dir(&t).join("a/s")).unwrap();
    let flat = SyncOptions { direction: SyncDirection::RemoteToLocal, by: SyncBy::Time, subfolders: false };
    let plan = scan(&mut t, session, flat).await;

    run(&mut t, &plan, ticked(&plan)).await;

    let paths: Vec<PathBuf> = plan.items.iter().map(|item| item.path.clone()).collect();
    assert_eq!(paths, vec![PathBuf::from("top.txt")]);
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
}
