use std::{path::PathBuf, time::Duration};

use super::*;
use crate::{
    engine::testing::{TestEngine, test_engine},
    transfer::{Direction, JobStatus, rows::RowKind},
};

fn queue_job(t: &mut TestEngine, session: u64, name: &str) -> u64 {
    t.engine.transfers.enqueue(
        session,
        Direction::Upload,
        PathBuf::from(format!("/local/{name}")),
        format!("/remote/{name}"),
        name.to_string(),
        10,
        None,
    )
}

fn snapshots(events: Vec<Event>) -> Vec<TransferSnapshot> {
    events
        .into_iter()
        .filter_map(|event| match event {
            Event::TransfersChanged(snapshot) => Some(snapshot),
            _ => None,
        })
        .collect()
}

#[test]
fn with_no_interval_every_request_publishes_at_once() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    queue_job(&mut t, session, "a");
    t.engine.request_publish();
    queue_job(&mut t, session, "b");
    t.engine.request_publish();

    assert_eq!(snapshots(t.drain()).len(), 2);
    assert!(!t.engine.transfers_dirty);
    assert!(t.engine.flush_deadline().is_none());
}

#[test]
fn a_burst_inside_the_interval_is_published_once_and_the_rest_waits_for_the_flush() {
    let mut t = test_engine();
    t.engine.publish_interval = Duration::from_millis(80);
    let (session, _fs) = t.add_session("prod");
    queue_job(&mut t, session, "a");
    t.engine.request_publish();
    queue_job(&mut t, session, "b");
    t.engine.request_publish();
    queue_job(&mut t, session, "c");
    t.engine.request_publish();

    let first = snapshots(t.drain());
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].rows.len(), 1);
    assert!(t.engine.transfers_dirty);
    assert!(t.engine.flush_deadline().is_some());

    std::thread::sleep(Duration::from_millis(100));
    t.engine.publish_transfers();

    let flushed = snapshots(t.drain());
    assert_eq!(flushed.len(), 1);
    assert_eq!(flushed[0].rows.len(), 3);
    assert!(!t.engine.transfers_dirty);
    assert!(t.engine.flush_deadline().is_none());
}

#[test]
fn a_request_after_a_quiet_period_publishes_at_once() {
    let mut t = test_engine();
    t.engine.publish_interval = Duration::from_millis(40);
    let (session, _fs) = t.add_session("prod");
    queue_job(&mut t, session, "a");
    t.engine.request_publish();
    t.drain();
    std::thread::sleep(Duration::from_millis(60));
    queue_job(&mut t, session, "b");

    t.engine.request_publish();

    assert_eq!(snapshots(t.drain()).len(), 1);
}

#[test]
fn nothing_is_published_when_nothing_changed() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    queue_job(&mut t, session, "a");
    t.engine.publish_transfers();
    t.drain();

    t.engine.publish_transfers();

    assert!(snapshots(t.drain()).is_empty());
}

#[tokio::test]
async fn the_run_loop_publishes_the_trailing_snapshot_of_a_burst() {
    let mut t = test_engine();
    t.engine.publish_interval = Duration::from_millis(60);
    let (session, _fs) = t.add_session("prod");
    let first = queue_job(&mut t, session, "a");
    let second = queue_job(&mut t, session, "b");
    queue_job(&mut t, session, "c");
    let TestEngine { engine, mut events, internal, dir } = t;
    let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
    let running = tokio::spawn(engine.run(receiver, internal));

    commands.send(Command::CancelRow { kind: RowKind::Single(first) }).unwrap();
    commands.send(Command::CancelRow { kind: RowKind::Single(second) }).unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    commands.send(Command::Shutdown).unwrap();
    running.await.unwrap();

    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let Event::TransfersChanged(snapshot) = event {
            seen.push(snapshot);
        }
    }
    assert_eq!(seen.len(), 2);
    let states: Vec<_> = seen[1].rows.iter().map(|row| row.state).collect();
    assert_eq!(
        states,
        vec![
            crate::transfer::rows::RowState::Cancelled,
            crate::transfer::rows::RowState::Cancelled,
            crate::transfer::rows::RowState::Queued
        ]
    );
    drop(dir);
}

#[test]
fn a_job_status_helper_still_reaches_the_published_snapshot() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = queue_job(&mut t, session, "a");
    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::InProgress;

    t.engine.publish_transfers();

    let published = snapshots(t.drain());
    assert_eq!(published[0].active.len(), 1);
}

#[tokio::test]
async fn shutting_down_while_a_snapshot_is_pending_publishes_it_first() {
    let mut t = test_engine();
    t.engine.publish_interval = Duration::from_secs(30);
    let (session, _fs) = t.add_session("prod");
    let first = queue_job(&mut t, session, "a");
    let second = queue_job(&mut t, session, "b");
    queue_job(&mut t, session, "c");
    let TestEngine { engine, mut events, internal, dir } = t;
    let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
    let running = tokio::spawn(engine.run(receiver, internal));

    commands.send(Command::CancelRow { kind: RowKind::Single(first) }).unwrap();
    commands.send(Command::CancelRow { kind: RowKind::Single(second) }).unwrap();
    commands.send(Command::Shutdown).unwrap();
    running.await.unwrap();

    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let Event::TransfersChanged(snapshot) = event {
            seen.push(snapshot);
        }
    }
    assert_eq!(seen.len(), 2);
    let states: Vec<_> = seen[1].rows.iter().map(|row| row.state).collect();
    assert_eq!(
        states,
        vec![
            crate::transfer::rows::RowState::Cancelled,
            crate::transfer::rows::RowState::Cancelled,
            crate::transfer::rows::RowState::Queued
        ]
    );
    drop(dir);
}
