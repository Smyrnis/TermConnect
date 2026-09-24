use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use super::*;
use crate::{
    engine::testing::{TestEngine, test_engine},
    transfer::{
        conflicts::ConflictPolicy,
        plan::{ExistingFile, PlannedFile},
        rows::RowState,
    },
};

fn planning_scan(batch_id: u64, session_id: u64, name: &str) -> PlanningScan {
    PlanningScan {
        batch_id,
        session_id,
        direction: Direction::Upload,
        display_name: name.to_string(),
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

fn planned(source: &str, destination: &str, size: u64) -> PlannedFile {
    PlannedFile {
        source: PathBuf::from(source),
        destination: PathBuf::from(destination),
        display_name: PathBuf::from(destination).file_name().unwrap().to_string_lossy().into_owned(),
        size,
        existing: None,
        source_modified: None,
        partial: None,
        resume: false,
    }
}

fn plan(files: Vec<PlannedFile>) -> DirectoryPlan {
    let names = files
        .iter()
        .filter_map(|file| {
            Some((file.destination.parent()?.to_path_buf(), file.destination.file_name()?.to_string_lossy().into()))
        })
        .fold(HashMap::<PathBuf, HashSet<String>>::new(), |mut taken, (parent, name)| {
            taken.entry(parent).or_default().insert(name);
            taken
        });
    DirectoryPlan { files, skipped_symlinks: 0, taken_names: names }
}

fn enqueue_job(t: &mut TestEngine, session_id: u64, name: &str, batch_id: Option<u64>) -> u64 {
    t.engine.transfers.enqueue(
        session_id,
        Direction::Upload,
        PathBuf::from(format!("/local/{name}")),
        format!("/remote/{name}"),
        name.to_string(),
        10,
        batch_id,
    )
}

fn download_job(t: &mut TestEngine, session_id: u64, name: &str) -> u64 {
    let local = t.dir.path().join(name);
    t.engine.transfers.enqueue(
        session_id,
        Direction::Download,
        local,
        format!("/remote/{name}"),
        name.to_string(),
        10,
        None,
    )
}

fn mark_active(t: &mut TestEngine, id: u64) -> Arc<AtomicBool> {
    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::InProgress;
    let cancel = Arc::new(AtomicBool::new(false));
    t.engine.transfer_cancels.insert(id, cancel.clone());
    cancel
}

fn local_changed(events: &[Event]) -> bool {
    events.iter().any(|event| matches!(event, Event::LocationChanged { location: Location::Local }))
}

fn local_entry(t: &TestEngine, name: &str, is_dir: bool) -> Entry {
    Entry { name: name.to_string(), path: t.dir.path().join(name), is_dir, size: 1, permissions: None }
}

#[test]
fn fill_transfer_slots_notifies_when_the_jobs_session_has_disconnected() {
    let mut t = test_engine();
    enqueue_job(&mut t, 999, "file.txt", None);

    t.engine.fill_transfer_slots();

    let (severity, message) = t.first_notice().unwrap();
    assert_eq!(severity, Severity::Error);
    assert!(message.contains("file.txt"));
    assert!(message.contains("disconnected"));
}

#[test]
fn copying_a_directory_with_a_disconnected_session_fails_without_spawning() {
    let mut t = test_engine();
    std::fs::create_dir(t.dir.path().join("myfolder")).unwrap();
    let entry = local_entry(&t, "myfolder", true);

    t.engine.copy(Location::Local, vec![entry], Location::Session(7), PathBuf::from("/remote"));

    assert!(t.engine.planning.is_empty());
    let (severity, message) = t.first_notice().unwrap();
    assert_eq!(severity, Severity::Error);
    assert!(message.contains("disconnected"));
    assert_eq!(t.engine.transfers.queued_count(), 0);
}

#[test]
fn copying_only_files_now_goes_through_the_scan() {
    let mut t = test_engine();
    std::fs::write(t.dir.path().join("a.txt"), b"x").unwrap();
    let entry = local_entry(&t, "a.txt", false);

    t.engine.copy(Location::Local, vec![entry], Location::Session(7), PathBuf::from("/remote"));

    assert_eq!(t.first_notice().unwrap().1, "Copy failed: session disconnected");
    assert_eq!(t.engine.transfers.jobs().count(), 0);
}

#[test]
fn copying_between_two_sessions_is_refused() {
    let mut t = test_engine();

    t.engine.copy(Location::Session(1), vec![], Location::Session(2), PathBuf::from("/"));

    assert_eq!(
        t.first_notice(),
        Some((Severity::Warning, "Copying between two remote sessions isn't supported yet".to_string()))
    );
}

#[tokio::test]
async fn an_upload_is_planned_copied_and_announced_end_to_end() {
    let mut t = test_engine();
    let (session, remote) = t.add_session("srv");
    remote.dir("/upload");
    std::fs::write(t.dir.path().join("a.txt"), b"hello").unwrap();
    let entry = local_entry(&t, "a.txt", false);

    t.engine.copy(Location::Local, vec![entry], Location::Session(session), PathBuf::from("/upload"));
    t.run_internal().await;
    while t.engine.transfers.jobs().any(|job| job.status != JobStatus::Completed) {
        t.run_internal().await;
    }

    assert_eq!(remote.contents("/upload/a.txt").unwrap(), b"hello");
    assert!(
        t.drain()
            .iter()
            .any(|event| matches!(event, Event::LocationChanged { location: Location::Session(id) } if *id == session))
    );
}

#[test]
fn plan_ready_enqueues_every_planned_file_under_the_batch_id() {
    let mut t = test_engine();
    let batch_id = t.engine.transfers.start_batch("batch".to_string());
    let plan = plan(vec![planned("/local/a.txt", "/remote/a.txt", 10), planned("/local/b.txt", "/remote/b.txt", 20)]);

    t.engine.apply_plan_ready(batch_id, 1, Direction::Upload, plan, &[]);

    let progress = t.engine.transfers.batch_progress(batch_id);
    assert_eq!(progress.total_files, 2);
    assert_eq!(progress.total_bytes, 30);
}

#[test]
fn plan_ready_warns_once_about_skipped_symlinks() {
    let mut t = test_engine();
    let batch_id = t.engine.transfers.start_batch("batch".to_string());
    let plan = DirectoryPlan { files: Vec::new(), skipped_symlinks: 3, taken_names: HashMap::new() };

    t.engine.apply_plan_ready(batch_id, 1, Direction::Upload, plan, &[]);

    let (severity, message) = t.first_notice().unwrap();
    assert_eq!(severity, Severity::Warning);
    assert!(message.contains("3 symlinks"));
}

#[test]
fn plan_ready_fails_without_enqueueing_when_the_session_has_disconnected() {
    let mut t = test_engine();
    let batch_id = t.engine.transfers.start_batch("batch".to_string());
    t.engine.planning.push(planning_scan(batch_id, 1, "myfolder"));

    t.engine.handle_transfer_event(TransferEvent::PlanReady {
        batch_id,
        session_id: 1,
        direction: Direction::Upload,
        plan: plan(vec![planned("/local/a.txt", "/remote/a.txt", 10)]),
    });

    assert!(t.engine.planning.is_empty());
    let (severity, message) = t.first_notice().unwrap();
    assert_eq!(severity, Severity::Error);
    assert!(message.contains("disconnected"));
    assert_eq!(t.engine.transfers.batch_progress(batch_id).total_files, 0);
}

#[test]
fn plan_failed_clears_planning_and_shows_an_error() {
    let mut t = test_engine();
    let batch_id = t.engine.transfers.start_batch("batch".to_string());
    t.engine.planning.push(planning_scan(batch_id, 1, "myfolder"));

    t.engine.handle_transfer_event(TransferEvent::PlanFailed {
        batch_id,
        message: "Copy failed: permission denied".to_string(),
    });

    assert!(t.engine.planning.is_empty());
    assert_eq!(t.first_notice(), Some((Severity::Error, "Copy failed: permission denied".to_string())));
}

#[test]
fn plan_cancelled_clears_planning_and_shows_an_info_notification() {
    let mut t = test_engine();
    let (session_id, _) = t.add_session("test");
    let batch_id = t.engine.transfers.start_batch("batch".to_string());
    t.engine.planning.push(planning_scan(batch_id, session_id, "myfolder"));

    t.engine.handle_transfer_event(TransferEvent::PlanCancelled { batch_id, session_id, direction: Direction::Upload });

    assert!(t.engine.planning.is_empty());
    assert_eq!(t.first_notice(), Some((Severity::Info, "Copy cancelled".to_string())));
    assert_eq!(t.engine.transfers.batch_progress(batch_id).total_files, 0);
}

#[test]
fn plan_cancelled_only_clears_its_own_scan() {
    let mut t = test_engine();
    let first = t.engine.transfers.start_batch("batch".to_string());
    let second = t.engine.transfers.start_batch("batch".to_string());
    t.engine.planning.push(planning_scan(first, 1, "one"));
    t.engine.planning.push(planning_scan(second, 1, "two"));

    t.engine.handle_transfer_event(TransferEvent::PlanCancelled {
        batch_id: first,
        session_id: 1,
        direction: Direction::Upload,
    });

    let remaining: Vec<u64> = t.engine.planning.iter().map(|scan| scan.batch_id).collect();
    assert_eq!(remaining, vec![second]);
}

#[test]
fn plan_cancelled_for_a_disconnected_session_is_silent() {
    let mut t = test_engine();
    let batch_id = t.engine.transfers.start_batch("batch".to_string());
    t.engine.planning.push(planning_scan(batch_id, 1, "myfolder"));

    t.engine.handle_transfer_event(TransferEvent::PlanCancelled {
        batch_id,
        session_id: 1,
        direction: Direction::Upload,
    });

    assert!(t.engine.planning.is_empty());
    assert!(t.first_notice().is_none());
}

#[test]
fn cancel_all_copies_sets_every_scans_flag_and_leaves_them_tracked() {
    let mut t = test_engine();
    t.engine.planning.push(planning_scan(0, 1, "one"));
    t.engine.planning.push(planning_scan(1, 1, "two"));

    t.engine.cancel_all_copies();

    assert!(t.engine.planning.iter().all(|scan| scan.cancel.load(Ordering::Relaxed)));
    assert_eq!(t.engine.planning.len(), 2);
}

#[test]
fn cancel_all_copies_stops_scans_and_the_active_batch_together() {
    let mut t = test_engine();
    t.engine.planning.push(planning_scan(100, 1, "scanning"));
    let batch_id = t.engine.transfers.start_batch("batch".to_string());
    let active = enqueue_job(&mut t, 1, "a.txt", Some(batch_id));
    let queued = enqueue_job(&mut t, 1, "b.txt", Some(batch_id));
    let transfer_cancel = mark_active(&mut t, active);

    t.engine.cancel_all_copies();

    assert!(t.engine.planning[0].cancel.load(Ordering::Relaxed));
    assert!(transfer_cancel.load(Ordering::Relaxed));
    assert_eq!(t.engine.transfers.get(queued).unwrap().status, JobStatus::Cancelled);
}

#[test]
fn cancel_all_copies_with_nothing_running_is_a_no_op() {
    let mut t = test_engine();

    t.engine.cancel_all_copies();

    assert!(t.engine.planning.is_empty());
    assert!(t.first_notice().is_none());
}

#[test]
fn fill_transfer_slots_fails_every_job_whose_session_is_gone_without_starting_any() {
    let mut t = test_engine();
    let jobs: Vec<u64> = (0..6).map(|n| enqueue_job(&mut t, 999, &format!("{n}.txt"), None)).collect();

    t.engine.fill_transfer_slots();

    assert!(jobs.iter().all(|id| matches!(t.engine.transfers.get(*id).unwrap().status, JobStatus::Failed(_))));
    assert!(t.engine.transfer_cancels.is_empty());
    assert_eq!(t.engine.transfers.active_count(), 0);
}

#[test]
fn cancel_all_copies_flags_every_active_job_and_cancels_every_queued_job() {
    let mut t = test_engine();
    let batch_id = t.engine.transfers.start_batch("batch".to_string());
    let first_active = enqueue_job(&mut t, 1, "a.txt", Some(batch_id));
    let second_active = enqueue_job(&mut t, 1, "b.txt", None);
    let queued_in_batch = enqueue_job(&mut t, 1, "c.txt", Some(batch_id));
    let queued_loose = enqueue_job(&mut t, 2, "d.txt", None);
    let first_cancel = mark_active(&mut t, first_active);
    let second_cancel = mark_active(&mut t, second_active);

    t.engine.cancel_all_copies();

    assert!(first_cancel.load(Ordering::Relaxed));
    assert!(second_cancel.load(Ordering::Relaxed));
    assert_eq!(t.engine.transfers.get(queued_in_batch).unwrap().status, JobStatus::Cancelled);
    assert_eq!(t.engine.transfers.get(queued_loose).unwrap().status, JobStatus::Cancelled);
    assert_eq!(t.engine.transfers.get(first_active).unwrap().status, JobStatus::InProgress);
}

#[test]
fn nothing_is_startable_after_cancel_all_copies() {
    let mut t = test_engine();
    let active = enqueue_job(&mut t, 1, "a.txt", None);
    enqueue_job(&mut t, 1, "b.txt", None);
    mark_active(&mut t, active);

    t.engine.cancel_all_copies();
    t.engine.handle_transfer_event(TransferEvent::Finished { id: active, outcome: TransferOutcome::Cancelled });

    assert!(t.engine.transfers.startable(t.engine.max_parallel).is_empty());
    assert!(t.first_notice().is_none());
}

#[test]
fn cancel_session_transfers_flags_only_that_sessions_jobs_and_scans() {
    let mut t = test_engine();
    let mine = enqueue_job(&mut t, 1, "a.txt", None);
    let my_cancel = mark_active(&mut t, mine);
    t.engine.planning.push(planning_scan(7, 1, "mine"));

    let flagged = t.engine.cancel_session_transfers(1);

    assert_eq!(flagged, 2);
    assert!(my_cancel.load(Ordering::Relaxed));
    assert!(t.engine.planning[0].cancel.load(Ordering::Relaxed));
}

#[test]
fn cancel_session_transfers_leaves_other_sessions_alone() {
    let mut t = test_engine();
    let theirs = enqueue_job(&mut t, 2, "a.txt", None);
    let their_cancel = mark_active(&mut t, theirs);
    t.engine.planning.push(planning_scan(7, 2, "theirs"));

    let flagged = t.engine.cancel_session_transfers(1);

    assert_eq!(flagged, 0);
    assert!(!their_cancel.load(Ordering::Relaxed));
    assert!(!t.engine.planning[0].cancel.load(Ordering::Relaxed));
}

#[test]
fn a_finished_event_removes_only_its_own_cancel_flag() {
    let mut t = test_engine();
    let first = enqueue_job(&mut t, 1, "a.txt", None);
    let second = enqueue_job(&mut t, 1, "b.txt", None);
    mark_active(&mut t, first);
    mark_active(&mut t, second);

    t.engine.handle_transfer_event(TransferEvent::Finished { id: first, outcome: TransferOutcome::Completed });

    assert!(!t.engine.transfer_cancels.contains_key(&first));
    assert!(t.engine.transfer_cancels.contains_key(&second));
}

#[test]
fn a_failed_event_removes_its_cancel_flag_and_requeues_within_the_retry_limit() {
    let mut t = test_engine();
    let job = enqueue_job(&mut t, 999, "a.txt", None);
    mark_active(&mut t, job);
    t.engine.transfers.get_mut(job).unwrap().attempts = 1;

    t.engine.handle_transfer_event(TransferEvent::Failed { id: job, message: "Transfer failed: a.txt".to_string() });

    assert!(!t.engine.transfer_cancels.contains_key(&job));
    assert!(
        matches!(t.engine.transfers.get(job).unwrap().status, JobStatus::Failed(ref reason) if reason == "session disconnected")
    );
}

#[test]
fn a_cancelled_job_that_ends_in_an_error_is_not_retried() {
    let mut t = test_engine();
    let job = enqueue_job(&mut t, 999, "a.txt", None);
    let cancel = mark_active(&mut t, job);
    cancel.store(true, Ordering::Relaxed);

    t.engine.handle_transfer_event(TransferEvent::Failed { id: job, message: "Transfer failed: a.txt".to_string() });

    assert_eq!(t.engine.transfers.get(job).unwrap().status, JobStatus::Cancelled);
    assert!(t.first_notice().is_none());
}

#[test]
fn a_permanent_failure_of_the_last_pending_job_refreshes_the_destination() {
    let mut t = test_engine();
    let done = download_job(&mut t, 1, "done.txt");
    t.engine.transfers.get_mut(done).unwrap().status = JobStatus::Completed;
    let failing = download_job(&mut t, 1, "bad.txt");
    mark_active(&mut t, failing);
    t.engine.transfers.get_mut(failing).unwrap().attempts = 3;

    t.engine
        .handle_transfer_event(TransferEvent::Failed { id: failing, message: "Transfer failed: bad.txt".to_string() });

    assert!(local_changed(&t.drain()));
}

#[test]
fn a_job_that_fails_to_start_refreshes_the_destination_once_nothing_is_pending() {
    let mut t = test_engine();
    let done = download_job(&mut t, 999, "done.txt");
    t.engine.transfers.get_mut(done).unwrap().status = JobStatus::Completed;
    download_job(&mut t, 999, "bad.txt");

    t.engine.fill_transfer_slots();

    assert!(local_changed(&t.drain()));
}

#[test]
fn a_finished_download_waits_to_refresh_while_another_download_of_that_session_runs() {
    let mut t = test_engine();
    let finished = download_job(&mut t, 1, "a.txt");
    let still_running = download_job(&mut t, 1, "b.txt");
    mark_active(&mut t, finished);
    mark_active(&mut t, still_running);

    t.engine.handle_transfer_event(TransferEvent::Finished { id: finished, outcome: TransferOutcome::Completed });

    assert!(!local_changed(&t.drain()));
}

#[test]
fn a_finished_download_refreshes_despite_other_sessions_and_upload_jobs() {
    let mut t = test_engine();
    let finished = download_job(&mut t, 1, "a.txt");
    mark_active(&mut t, finished);
    let other_session_download = download_job(&mut t, 2, "b.txt");
    mark_active(&mut t, other_session_download);
    let same_session_upload = enqueue_job(&mut t, 1, "c.txt", None);
    mark_active(&mut t, same_session_upload);

    t.engine.handle_transfer_event(TransferEvent::Finished { id: finished, outcome: TransferOutcome::Completed });

    assert!(local_changed(&t.drain()));
}

#[test]
fn cancel_all_copies_refreshes_destinations_left_with_nothing_pending() {
    let mut t = test_engine();
    let done = download_job(&mut t, 1, "done.txt");
    t.engine.transfers.get_mut(done).unwrap().status = JobStatus::Completed;
    download_job(&mut t, 1, "later.txt");

    t.engine.cancel_all_copies();

    assert!(local_changed(&t.drain()));
}

#[test]
fn plan_failed_and_cancelled_forget_the_batch_label() {
    let mut t = test_engine();
    let failed = t.engine.transfers.start_batch("one".to_string());
    let cancelled = t.engine.transfers.start_batch("two".to_string());

    t.engine.handle_transfer_event(TransferEvent::PlanFailed {
        batch_id: failed,
        message: "Copy failed: boom".to_string(),
    });
    t.engine.handle_transfer_event(TransferEvent::PlanCancelled {
        batch_id: cancelled,
        session_id: 1,
        direction: Direction::Upload,
    });

    assert_eq!(t.engine.transfers.batch_label(failed), None);
    assert_eq!(t.engine.transfers.batch_label(cancelled), None);
}

#[test]
fn plan_ready_for_a_disconnected_session_forgets_the_batch_label() {
    let mut t = test_engine();
    let batch_id = t.engine.transfers.start_batch("photos".to_string());

    t.engine.handle_transfer_event(TransferEvent::PlanReady {
        batch_id,
        session_id: 1,
        direction: Direction::Upload,
        plan: plan(Vec::new()),
    });

    assert_eq!(t.engine.transfers.batch_label(batch_id), None);
}

#[test]
fn a_ready_plan_keeps_its_label_only_when_it_has_files() {
    let mut t = test_engine();
    let with_files = t.engine.transfers.start_batch("photos".to_string());
    let empty = t.engine.transfers.start_batch("empty".to_string());

    t.engine.apply_plan_ready(
        with_files,
        1,
        Direction::Upload,
        plan(vec![planned("/local/a.txt", "/remote/a.txt", 10)]),
        &[],
    );
    t.engine.apply_plan_ready(empty, 1, Direction::Upload, plan(Vec::new()), &[]);

    assert_eq!(t.engine.transfers.batch_label(with_files), Some("photos"));
    assert_eq!(t.engine.transfers.batch_label(empty), None);
}

#[test]
fn progress_events_update_the_job_and_are_published_at_most_every_interval() {
    let mut t = test_engine();
    let job = enqueue_job(&mut t, 1, "a.txt", None);
    mark_active(&mut t, job);
    t.engine.publish_transfers();
    t.drain();

    t.engine.handle_internal(Internal::Transfer(TransferEvent::Progress { id: job, transferred: 5 }));
    t.engine.handle_internal(Internal::Transfer(TransferEvent::Progress { id: job, transferred: 10 }));

    assert_eq!(t.engine.transfers.get(job).unwrap().transferred_bytes, 10);
    let snapshots = t.drain().into_iter().filter(|event| matches!(event, Event::TransfersChanged(_))).count();
    assert_eq!(snapshots, 0, "progress right after a publish is held back");
    t.engine
        .handle_internal(Internal::Transfer(TransferEvent::Finished { id: job, outcome: TransferOutcome::Completed }));
    assert!(
        t.drain().iter().any(|event| matches!(event, Event::TransfersChanged(snapshot) if snapshot.active.is_empty()))
    );
}

fn conflict_file(name: &str, existing: bool) -> PlannedFile {
    let mut file = planned(&format!("/local/{name}"), &format!("/remote/{name}"), 1);
    file.existing = existing.then_some(ExistingFile { size: 2, modified: None, is_dir: false });
    file
}

fn queued_remote_paths(t: &TestEngine) -> Vec<String> {
    t.engine.transfers.jobs().map(|job| job.remote_path.clone()).collect()
}

fn review(t: &mut TestEngine, files: Vec<PlannedFile>) -> u64 {
    let batch_id = t.engine.transfers.start_batch("copy".to_string());
    t.engine.review_or_apply_plan(batch_id, 1, Direction::Upload, plan(files));
    batch_id
}

#[test]
fn a_plan_without_conflicts_is_queued_without_a_prompt() {
    let mut t = test_engine();

    review(&mut t, vec![conflict_file("a.txt", false)]);

    assert!(!t.drain().iter().any(|event| matches!(event, Event::ConflictsFound { .. })));
    assert_eq!(queued_remote_paths(&t), vec!["/remote/a.txt".to_string()]);
}

#[test]
fn conflicts_open_a_prompt_and_answers_decide_what_is_queued() {
    let mut t = test_engine();
    let batch_id =
        review(&mut t, vec![conflict_file("a.txt", false), conflict_file("b.txt", true), conflict_file("c.txt", true)]);

    let files = t
        .drain()
        .into_iter()
        .find_map(|event| match event {
            Event::ConflictsFound { files, .. } => Some(files),
            _ => None,
        })
        .unwrap();
    assert_eq!(files.iter().map(|file| file.display_name.as_str()).collect::<Vec<_>>(), ["b.txt", "c.txt"]);

    t.engine.resolve_conflicts(batch_id, Some(vec![Resolution::Skip, Resolution::Rename]));

    assert_eq!(queued_remote_paths(&t), vec!["/remote/a.txt".to_string(), "/remote/c (1).txt".to_string()]);
}

#[test]
fn cancelling_the_copy_queues_nothing_and_says_so() {
    let mut t = test_engine();
    let batch_id = review(&mut t, vec![conflict_file("a.txt", false), conflict_file("b.txt", true)]);

    t.engine.resolve_conflicts(batch_id, None);

    assert_eq!(t.engine.transfers.jobs().count(), 0);
    assert_eq!(t.first_notice().unwrap().1, "Copy cancelled");
    assert_eq!(t.engine.transfers.batch_label(batch_id), None);
}

#[test]
fn a_policy_other_than_ask_never_prompts() {
    let mut t = test_engine();
    t.engine.on_conflict = ConflictPolicy::Skip;

    review(&mut t, vec![conflict_file("a.txt", false), conflict_file("b.txt", true)]);

    assert!(!t.drain().iter().any(|event| matches!(event, Event::ConflictsFound { .. })));
    assert_eq!(queued_remote_paths(&t), vec!["/remote/a.txt".to_string()]);
}

#[test]
fn skipped_existing_files_are_reported() {
    let mut t = test_engine();
    t.engine.on_conflict = ConflictPolicy::Skip;

    review(&mut t, vec![conflict_file("a.txt", false), conflict_file("b.txt", true), conflict_file("c.txt", true)]);

    assert_eq!(t.first_notice(), Some((Severity::Info, "Skipped 2 existing files".to_string())));
}

#[test]
fn files_blocked_by_a_folder_are_reported() {
    let mut t = test_engine();
    t.engine.on_conflict = ConflictPolicy::Overwrite;
    let mut blocked = conflict_file("photos", true);
    blocked.existing = Some(ExistingFile { size: 0, modified: None, is_dir: true });

    review(&mut t, vec![blocked]);

    assert_eq!(
        t.first_notice(),
        Some((Severity::Warning, "Skipped 1 file because a folder with the same name exists".to_string()))
    );
}

#[test]
fn an_automatic_policy_resumes_a_partial_and_queues_it_with_resume() {
    let mut t = test_engine();
    t.engine.on_conflict = ConflictPolicy::Skip;
    let mut partial = conflict_file("big.iso", false);
    partial.partial = Some(ExistingFile { size: 0, modified: None, is_dir: false });

    review(&mut t, vec![partial]);

    assert!(t.engine.transfers.jobs().next().unwrap().resume);
}

#[test]
fn skipping_a_partial_is_reported_as_partly_copied() {
    let mut t = test_engine();
    let mut partial = conflict_file("a.iso", false);
    partial.partial = Some(ExistingFile { size: 0, modified: None, is_dir: false });
    let batch_id = review(&mut t, vec![partial]);
    t.drain();

    t.engine.resolve_conflicts(batch_id, Some(vec![Resolution::Skip]));

    assert_eq!(t.first_notice().unwrap().1, "Skipped 1 partly copied file");
}

#[test]
fn a_waiting_copy_shows_on_the_transfers_screen() {
    let mut t = test_engine();
    review(&mut t, vec![conflict_file("a.txt", true)]);

    let rows = t.engine.snapshot().rows;

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, RowState::AwaitingAnswer);
    assert_eq!(rows[0].label, "copy");
}

fn add_job(t: &mut TestEngine, name: &str, batch_id: Option<u64>, status: JobStatus) -> u64 {
    let id = enqueue_job(t, 999, name, batch_id);
    t.engine.transfers.get_mut(id).unwrap().status = status;
    id
}

fn flag(t: &mut TestEngine, id: u64) -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    t.engine.transfer_cancels.insert(id, cancel.clone());
    cancel
}

#[test]
fn cancel_on_a_batch_row_cancels_only_that_batch() {
    let mut t = test_engine();
    let batch = t.engine.transfers.start_batch("photos".to_string());
    let queued = add_job(&mut t, "a", Some(batch), JobStatus::Queued);
    let running = add_job(&mut t, "b", Some(batch), JobStatus::InProgress);
    let running_flag = flag(&mut t, running);
    let completed = add_job(&mut t, "c", Some(batch), JobStatus::Completed);
    let other_batch = t.engine.transfers.start_batch("docs".to_string());
    let other = add_job(&mut t, "d", Some(other_batch), JobStatus::Queued);

    t.engine.cancel_row(RowKind::Batch(batch));

    assert_eq!(t.engine.transfers.get(queued).unwrap().status, JobStatus::Cancelled);
    assert!(running_flag.load(Ordering::Relaxed));
    assert_eq!(t.engine.transfers.get(running).unwrap().status, JobStatus::InProgress);
    assert_eq!(t.engine.transfers.get(completed).unwrap().status, JobStatus::Completed);
    assert_eq!(t.engine.transfers.get(other).unwrap().status, JobStatus::Queued);
}

#[test]
fn cancel_on_a_scan_row_flags_only_that_scan() {
    let mut t = test_engine();
    t.engine.planning.push(planning_scan(1, 1, "one"));
    t.engine.planning.push(planning_scan(2, 1, "two"));

    t.engine.cancel_row(RowKind::Scan(2));

    assert!(!t.engine.planning[0].cancel.load(Ordering::Relaxed));
    assert!(t.engine.planning[1].cancel.load(Ordering::Relaxed));
}

#[test]
fn cancel_on_a_waiting_copy_withdraws_its_prompt() {
    let mut t = test_engine();
    let batch_id = review(&mut t, vec![conflict_file("a.txt", true)]);
    t.drain();

    t.engine.cancel_row(RowKind::Scan(batch_id));

    assert!(t.engine.reviews.is_empty());
    assert!(
        t.drain()
            .iter()
            .any(|event| matches!(event, Event::ConflictsWithdrawn { batch_ids } if batch_ids == &vec![batch_id]))
    );
}

#[test]
fn cancel_on_a_finished_row_does_nothing() {
    let mut t = test_engine();
    let done = add_job(&mut t, "a", None, JobStatus::Completed);

    t.engine.cancel_row(RowKind::Single(done));

    assert_eq!(t.engine.transfers.get(done).unwrap().status, JobStatus::Completed);
    assert!(t.first_notice().is_none());
}

#[test]
fn retry_on_a_disconnected_session_shows_one_warning_and_leaves_the_files() {
    let mut t = test_engine();
    let batch = t.engine.transfers.start_batch("photos".to_string());
    let failed = add_job(&mut t, "a", Some(batch), JobStatus::Failed("boom".to_string()));
    t.engine.transfers.get_mut(failed).unwrap().attempts = 3;
    let cancelled = add_job(&mut t, "b", Some(batch), JobStatus::Cancelled);

    t.engine.retry_row(RowKind::Batch(batch));

    let job = t.engine.transfers.get(failed).unwrap();
    assert_eq!((job.status.clone(), job.attempts), (JobStatus::Failed("boom".to_string()), 3));
    assert_eq!(t.engine.transfers.get(cancelled).unwrap().status, JobStatus::Cancelled);
    assert_eq!(t.notices(), vec![(Severity::Warning, "Can't retry: session disconnected".to_string())]);
}

#[test]
fn cancelling_the_last_pending_row_refreshes_its_destination() {
    let mut t = test_engine();
    let done = download_job(&mut t, 1, "done.txt");
    t.engine.transfers.get_mut(done).unwrap().status = JobStatus::Completed;
    let later = download_job(&mut t, 1, "later.txt");

    t.engine.cancel_row(RowKind::Single(later));

    assert!(local_changed(&t.drain()));
}

#[test]
fn retry_leaves_running_jobs_alone() {
    let mut t = test_engine();
    let batch = t.engine.transfers.start_batch("photos".to_string());
    let running = add_job(&mut t, "a", Some(batch), JobStatus::InProgress);
    t.engine.transfers.get_mut(running).unwrap().attempts = 1;
    add_job(&mut t, "b", Some(batch), JobStatus::Failed("boom".to_string()));

    t.engine.retry_row(RowKind::Batch(batch));

    let job = t.engine.transfers.get(running).unwrap();
    assert_eq!((job.status.clone(), job.attempts), (JobStatus::InProgress, 1));
}

#[test]
fn clear_removes_only_finished_rows() {
    let mut t = test_engine();
    let done = add_job(&mut t, "a", None, JobStatus::Completed);
    let running = add_job(&mut t, "b", None, JobStatus::InProgress);
    let batch = t.engine.transfers.start_batch("photos".to_string());
    let failed_in_batch = add_job(&mut t, "c", Some(batch), JobStatus::Failed("boom".to_string()));

    t.engine.clear_finished_rows();

    assert!(t.engine.transfers.get(done).is_none());
    assert!(t.engine.transfers.get(failed_in_batch).is_none());
    assert!(t.engine.transfers.get(running).is_some());
    assert_eq!(t.engine.snapshot().rows.len(), 1);
}

#[test]
fn clear_keeps_a_scanning_copy() {
    let mut t = test_engine();
    let batch = t.engine.transfers.start_batch("photos".to_string());
    t.engine.planning.push(planning_scan(batch, 1, "photos"));
    add_job(&mut t, "a", None, JobStatus::Completed);

    t.engine.clear_finished_rows();

    assert_eq!(t.engine.snapshot().rows.len(), 1);
    assert_eq!(t.engine.transfers.batch_label(batch), Some("photos"));
}

#[test]
fn clearing_removes_partials_only_of_cancelled_and_failed_downloads() {
    let mut t = test_engine();
    for (name, status) in [
        ("cancelled.bin", JobStatus::Cancelled),
        ("failed.bin", JobStatus::Failed("boom".to_string())),
        ("done.bin", JobStatus::Completed),
        ("queued.bin", JobStatus::Queued),
    ] {
        let id = download_job(&mut t, 999, name);
        t.engine.transfers.get_mut(id).unwrap().status = status;
        std::fs::write(t.dir.path().join(format!("{name}.part")), b"x").unwrap();
    }

    t.engine.clear_finished_rows();

    assert!(!t.dir.path().join("cancelled.bin.part").exists());
    assert!(!t.dir.path().join("failed.bin.part").exists());
    assert!(t.dir.path().join("done.bin.part").exists());
    assert!(t.dir.path().join("queued.bin.part").exists());
}

#[test]
fn clearing_keeps_a_partial_that_a_queued_copy_of_the_same_file_uses() {
    let mut t = test_engine();
    for status in [JobStatus::Cancelled, JobStatus::Queued] {
        let id = download_job(&mut t, 999, "big.iso");
        t.engine.transfers.get_mut(id).unwrap().status = status;
    }
    std::fs::write(t.dir.path().join("big.iso.part"), b"x").unwrap();

    t.engine.clear_finished_rows();

    assert!(t.dir.path().join("big.iso.part").exists());
}

#[test]
fn clearing_never_deletes_a_completed_file_named_like_a_partial() {
    let mut t = test_engine();
    let cancelled = download_job(&mut t, 999, "foo");
    t.engine.transfers.get_mut(cancelled).unwrap().status = JobStatus::Cancelled;
    let completed = download_job(&mut t, 999, "foo.part");
    t.engine.transfers.get_mut(completed).unwrap().status = JobStatus::Completed;
    std::fs::write(t.dir.path().join("foo.part"), b"a real file").unwrap();

    t.engine.clear_finished_rows();

    assert!(t.dir.path().join("foo.part").exists());
}

#[tokio::test]
async fn clearing_removes_a_cancelled_uploads_remote_partial_and_refreshes_that_session() {
    let mut t = test_engine();
    let (session, remote) = t.add_session("srv");
    remote.file("/remote/up.bin.part", b"half", None);
    let id = enqueue_job(&mut t, session, "up.bin", None);
    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::Cancelled;

    t.engine.clear_finished_rows();
    t.run_internal().await;

    assert!(!remote.exists("/remote/up.bin.part"));
    assert!(
        t.drain()
            .iter()
            .any(|event| matches!(event, Event::LocationChanged { location: Location::Session(id) } if *id == session))
    );
}
