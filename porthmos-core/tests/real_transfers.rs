mod scp_rig;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use porthmos_core::{
    Command, Event,
    transfer::{
        conflicts::{ConflictPolicy, Resolution},
        rows::RowState,
    },
};
use scp_rig::{Rig, data, rig};

fn no_conflicts(_: &[porthmos_core::transfer::conflicts::ConflictInfo]) -> Option<Vec<Resolution>> {
    panic!("no conflict was expected")
}

async fn upload_ten_files(max_parallel: usize) -> usize {
    let mut rig = rig(max_parallel, ConflictPolicy::Ask).await;
    std::fs::create_dir(rig.local("up")).unwrap();
    for index in 0..10u8 {
        std::fs::write(rig.local(&format!("up/f{index}")), data(2_000_000, index)).unwrap();
    }
    rig.upload(&[("up", true)]);

    assert_eq!(rig.settle(no_conflicts).await, vec![RowState::Done]);
    for index in 0..10u8 {
        assert_eq!(std::fs::read(rig.remote(&format!("up/f{index}"))).unwrap(), data(2_000_000, index));
    }
    rig.max_active
}

#[tokio::test(flavor = "multi_thread")]
async fn uploads_run_at_most_max_parallel_at_once() {
    assert_eq!(upload_ten_files(3).await, 3);
    assert_eq!(upload_ten_files(1).await, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_run_at_most_max_parallel_at_once() {
    let mut rig = rig(4, ConflictPolicy::Ask).await;
    std::fs::create_dir(rig.remote("down")).unwrap();
    for index in 0..10u8 {
        std::fs::write(rig.remote(&format!("down/f{index}")), data(2_000_000, index)).unwrap();
    }
    rig.download(&[("down", true)]);

    assert_eq!(rig.settle(no_conflicts).await, vec![RowState::Done]);
    for index in 0..10u8 {
        assert_eq!(std::fs::read(rig.local(&format!("down/f{index}"))).unwrap(), data(2_000_000, index));
    }
    assert_eq!(rig.max_active, 4);
}

async fn upload_conflict(answer: Option<Resolution>) -> Rig {
    let mut rig = rig(4, ConflictPolicy::Ask).await;
    std::fs::write(rig.local("a.txt"), b"new").unwrap();
    std::fs::write(rig.local("b.txt"), b"bee").unwrap();
    std::fs::write(rig.remote("a.txt"), b"old").unwrap();
    rig.upload(&[("a.txt", false), ("b.txt", false)]);
    rig.settle(move |files| {
        assert_eq!(files.iter().map(|file| file.display_name.as_str()).collect::<Vec<_>>(), vec!["a.txt"]);
        assert_eq!(files[0].existing.map(|existing| existing.size), Some(3));
        answer.map(|answer| vec![answer])
    })
    .await;
    rig
}

#[tokio::test(flavor = "multi_thread")]
async fn an_upload_conflict_is_asked_and_each_answer_is_honoured() {
    let skipped = upload_conflict(Some(Resolution::Skip)).await;
    assert_eq!(std::fs::read(skipped.remote("a.txt")).unwrap(), b"old");
    assert_eq!(std::fs::read(skipped.remote("b.txt")).unwrap(), b"bee");

    let overwritten = upload_conflict(Some(Resolution::Overwrite)).await;
    assert_eq!(std::fs::read(overwritten.remote("a.txt")).unwrap(), b"new");

    let renamed = upload_conflict(Some(Resolution::Rename)).await;
    assert_eq!(std::fs::read(renamed.remote("a.txt")).unwrap(), b"old");
    assert_eq!(std::fs::read(renamed.remote("a (1).txt")).unwrap(), b"new");

    let mut cancelled = upload_conflict(None).await;
    cancelled.drain().await;
    assert!(!cancelled.remote("b.txt").exists());
    assert!(cancelled.notices.iter().any(|notice| notice == "Copy cancelled"), "{:?}", cancelled.notices);
}

async fn download_conflict(answer: Resolution) -> Rig {
    let mut rig = rig(4, ConflictPolicy::Ask).await;
    std::fs::write(rig.remote("a.txt"), b"new").unwrap();
    std::fs::write(rig.local("a.txt"), b"old").unwrap();
    rig.download(&[("a.txt", false)]);
    rig.settle(move |files| {
        assert_eq!(files.len(), 1);
        Some(vec![answer])
    })
    .await;
    rig
}

#[tokio::test(flavor = "multi_thread")]
async fn a_download_conflict_is_asked_and_each_answer_is_honoured() {
    assert_eq!(std::fs::read(download_conflict(Resolution::Skip).await.local("a.txt")).unwrap(), b"old");
    assert_eq!(std::fs::read(download_conflict(Resolution::Overwrite).await.local("a.txt")).unwrap(), b"new");
    let renamed = download_conflict(Resolution::Rename).await;
    assert_eq!(std::fs::read(renamed.local("a (1).txt")).unwrap(), b"new");
    assert_eq!(std::fs::read(renamed.local("a.txt")).unwrap(), b"old");
}

#[tokio::test(flavor = "multi_thread")]
async fn conflict_policies_answer_without_asking() {
    let cases = [
        (ConflictPolicy::Overwrite, &b"new"[..], false),
        (ConflictPolicy::Skip, &b"old"[..], false),
        (ConflictPolicy::Rename, &b"old"[..], true),
    ];
    for (policy, expected, renamed) in cases {
        let mut rig = rig(4, policy).await;
        std::fs::write(rig.local("a.txt"), b"new").unwrap();
        std::fs::write(rig.remote("a.txt"), b"old").unwrap();
        rig.upload(&[("a.txt", false)]);
        rig.settle(no_conflicts).await;

        assert_eq!(std::fs::read(rig.remote("a.txt")).unwrap(), expected, "{policy:?}");
        assert_eq!(rig.remote("a (1).txt").exists(), renamed, "{policy:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_in_the_way_is_never_overwritten() {
    let mut rig = rig(4, ConflictPolicy::Overwrite).await;
    std::fs::write(rig.local("a.txt"), b"new").unwrap();
    std::fs::create_dir(rig.remote("a.txt")).unwrap();
    std::fs::write(rig.remote("a.txt/keep"), b"k").unwrap();
    rig.upload(&[("a.txt", false)]);
    rig.settle(no_conflicts).await;

    assert_eq!(std::fs::read(rig.remote("a.txt/keep")).unwrap(), b"k");
    assert!(
        rig.notices.iter().any(|notice| notice.contains("a folder with the same name exists")),
        "{:?}",
        rig.notices
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_conflict_is_asked_as_a_folder() {
    let mut rig = rig(4, ConflictPolicy::Ask).await;
    std::fs::write(rig.local("a.txt"), b"new").unwrap();
    std::fs::create_dir(rig.remote("a.txt")).unwrap();
    rig.upload(&[("a.txt", false)]);
    rig.settle(|files| {
        assert!(files[0].existing.is_some_and(|existing| existing.is_dir));
        Some(vec![Resolution::Rename])
    })
    .await;

    assert!(rig.remote("a.txt").is_dir());
    assert_eq!(std::fs::read(rig.remote("a (1).txt")).unwrap(), b"new");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_upload_is_offered_as_resume_and_completes() {
    let mut rig = rig(4, ConflictPolicy::Ask).await;
    let full = data(3_000_000, 7);
    std::fs::write(rig.local("big.bin"), &full).unwrap();
    std::fs::write(rig.remote("big.bin.part"), &full[..1_000_000]).unwrap();
    rig.upload(&[("big.bin", false)]);
    let asked = Arc::new(Mutex::new(false));
    let seen = asked.clone();
    rig.settle(move |files| {
        *seen.lock().unwrap() = true;
        assert_eq!(files[0].partial.map(|partial| partial.size), Some(1_000_000));
        Some(vec![Resolution::Resume])
    })
    .await;

    assert!(*asked.lock().unwrap());
    assert_eq!(std::fs::read(rig.remote("big.bin")).unwrap(), full);
    assert!(!rig.remote("big.bin.part").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_everything_stops_the_copy_and_clearing_removes_partials() {
    let mut rig = rig(2, ConflictPolicy::Ask).await;
    std::fs::create_dir(rig.local("up")).unwrap();
    for index in 0..6u8 {
        std::fs::write(rig.local(&format!("up/f{index}")), data(30_000_000, index)).unwrap();
    }
    rig.upload(&[("up", true)]);
    loop {
        if let Event::TransfersChanged(snapshot) = rig.event().await
            && snapshot.active.iter().any(|job| job.transferred_bytes > 0)
        {
            break;
        }
    }

    rig.core.send(Command::CancelAllTransfers);
    let states = rig.settle(|_| None).await;

    assert!(states.iter().all(|state| *state == RowState::Cancelled), "{states:?}");
    assert_eq!((0..6u8).filter(|index| rig.remote(&format!("up/f{index}")).exists()).count(), 0);
    rig.core.send(Command::ClearFinished);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let left: Vec<_> = std::fs::read_dir(rig.remote("up")).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert!(left.is_empty(), "{left:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_scp_session_runs_no_more_transfers_than_it_can_carry() {
    assert_eq!(upload_ten_files(16).await, 6);
}
