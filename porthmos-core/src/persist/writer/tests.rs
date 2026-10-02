use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use super::*;
use crate::tasks::Tasks;

type Failures = Arc<Mutex<Vec<(PathBuf, String)>>>;

fn writer_with(interval: Duration) -> (Writer, Failures) {
    let failures = Arc::new(Mutex::new(Vec::new()));
    let sink = failures.clone();
    let writer = Writer::new(Tasks::new(|_, _, _| {}), interval, move |failure| {
        sink.lock().unwrap().push((failure.path, failure.message));
    });
    (writer, failures)
}

fn text(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap()
}

async fn wait_for_writes(writer: &Writer, count: usize) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while writer.writes_done() < count {
        assert!(std::time::Instant::now() < deadline, "{} writes, wanted {count}", writer.writes_done());
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[test]
fn without_a_runtime_the_write_happens_inline_and_reports_its_error() {
    let dir = tempfile::tempdir().unwrap();
    let (writer, _) = writer_with(Duration::from_millis(100));
    let path = dir.path().join("inline.toml");

    assert!(writer.write(path.clone(), b"one".to_vec(), 0o600).is_ok());
    assert_eq!(text(&path), "one");

    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    assert!(writer.write(blocker.join("nested"), b"two".to_vec(), 0o600).is_err());
}

#[tokio::test]
async fn a_queued_write_lands_after_a_flush() {
    let dir = tempfile::tempdir().unwrap();
    let (writer, _) = writer_with(Duration::from_millis(500));
    let path = dir.path().join("queued.toml");

    assert!(writer.write(path.clone(), b"hello".to_vec(), 0o600).is_ok());
    writer.flush().await;

    assert_eq!(text(&path), "hello");
}

#[tokio::test]
async fn only_the_latest_contents_of_a_burst_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (writer, _) = writer_with(Duration::from_secs(30));
    let path = dir.path().join("burst.toml");

    for index in 0..20 {
        writer.write(path.clone(), format!("version {index}").into_bytes(), 0o600).unwrap();
        tokio::task::yield_now().await;
    }
    writer.flush().await;

    assert_eq!(text(&path), "version 19");
    assert!(writer.writes_done() <= 2, "{} writes for one burst", writer.writes_done());
}

#[tokio::test]
async fn a_path_is_written_at_most_once_per_interval() {
    let dir = tempfile::tempdir().unwrap();
    let (writer, _) = writer_with(Duration::from_millis(1500));
    let path = dir.path().join("paced.toml");

    writer.write(path.clone(), b"first".to_vec(), 0o600).unwrap();
    wait_for_writes(&writer, 1).await;
    writer.write(path.clone(), b"second".to_vec(), 0o600).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(writer.writes_done(), 1);
    assert_eq!(text(&path), "first");
    wait_for_writes(&writer, 2).await;
    assert_eq!(text(&path), "second");
}

#[tokio::test]
async fn different_paths_do_not_hold_each_other_back() {
    let dir = tempfile::tempdir().unwrap();
    let (writer, _) = writer_with(Duration::from_secs(30));
    let first = dir.path().join("a.toml");
    let second = dir.path().join("b.toml");

    writer.write(first.clone(), b"a".to_vec(), 0o600).unwrap();
    writer.write(second.clone(), b"b".to_vec(), 0o600).unwrap();
    wait_for_writes(&writer, 2).await;

    assert_eq!(text(&first), "a");
    assert_eq!(text(&second), "b");
}

#[tokio::test]
async fn a_failed_write_is_reported_with_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let (writer, failures) = writer_with(Duration::ZERO);
    let path = blocker.join("file.toml");

    writer.write(path.clone(), b"data".to_vec(), 0o600).unwrap();
    writer.flush().await;

    let failures = failures.lock().unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, path);
    assert!(failures[0].1.contains("blocker"), "{}", failures[0].1);
}

#[tokio::test]
async fn private_files_stay_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let (writer, _) = writer_with(Duration::ZERO);
    let path = dir.path().join("private.toml");

    writer.write(path.clone(), b"secret".to_vec(), 0o600).unwrap();
    writer.flush().await;

    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
}

#[tokio::test]
async fn closing_flushes_what_is_pending_and_later_writes_happen_inline() {
    let dir = tempfile::tempdir().unwrap();
    let (writer, _) = writer_with(Duration::from_secs(5));
    let pending = dir.path().join("pending.toml");
    let later = dir.path().join("later.toml");
    writer.write(pending.clone(), b"pending".to_vec(), 0o600).unwrap();
    writer.write(pending.clone(), b"newest".to_vec(), 0o600).unwrap();

    writer.close();
    writer.write(later.clone(), b"inline".to_vec(), 0o600).unwrap();
    wait_for_writes(&writer, 2).await;

    assert_eq!(text(&pending), "newest");
    assert_eq!(text(&later), "inline");
}

#[tokio::test]
async fn flushing_with_nothing_queued_returns_at_once() {
    let (writer, _) = writer_with(Duration::from_secs(5));

    tokio::time::timeout(Duration::from_millis(500), writer.flush()).await.unwrap();
}

#[tokio::test]
async fn a_write_held_back_by_the_interval_is_written_by_write_pending_now() {
    let dir = tempfile::tempdir().unwrap();
    let (writer, _) = writer_with(Duration::from_secs(30));
    let path = dir.path().join("held.toml");
    writer.write(path.clone(), b"first".to_vec(), 0o600).unwrap();
    wait_for_writes(&writer, 1).await;
    writer.write(path.clone(), b"second".to_vec(), 0o600).unwrap();

    writer.write_pending_now();

    assert_eq!(text(&path), "second");
}

#[tokio::test]
async fn write_pending_now_with_nothing_pending_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (writer, _) = writer_with(Duration::from_secs(30));

    writer.write_pending_now();

    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    assert_eq!(writer.writes_done(), 0);
}

#[tokio::test]
async fn a_write_after_the_task_died_is_not_lost() {
    let dir = tempfile::tempdir().unwrap();
    let tasks = Tasks::new(|_, _, _| {});
    let writer = Writer::new(tasks.clone(), Duration::ZERO, |_| {});
    let first = dir.path().join("first.toml");
    let second = dir.path().join("second.toml");
    writer.write(first.clone(), b"one".to_vec(), 0o600).unwrap();
    writer.flush().await;
    tasks.abort_all();

    writer.write(second.clone(), b"two".to_vec(), 0o600).unwrap();
    writer.flush().await;

    assert_eq!(text(&second), "two");
}

#[tokio::test]
async fn pending_writes_survive_the_task_being_aborted() {
    let dir = tempfile::tempdir().unwrap();
    let tasks = Tasks::new(|_, _, _| {});
    let writer = Writer::new(tasks.clone(), Duration::from_secs(30), |_| {});
    let path = dir.path().join("kept.toml");
    writer.write(path.clone(), b"first".to_vec(), 0o600).unwrap();
    wait_for_writes(&writer, 1).await;
    writer.write(path.clone(), b"second".to_vec(), 0o600).unwrap();
    tasks.abort_all();
    tokio::task::yield_now().await;

    writer.close();

    assert_eq!(text(&path), "second");
}

#[tokio::test]
async fn a_writer_that_is_dropped_flushes_and_lets_its_task_end() {
    let dir = tempfile::tempdir().unwrap();
    let tasks = Tasks::new(|_, _, _| {});
    let writer = Writer::new(tasks.clone(), Duration::from_secs(30), |_| {});
    let path = dir.path().join("dropped.toml");
    writer.write(path.clone(), b"first".to_vec(), 0o600).unwrap();
    wait_for_writes(&writer, 1).await;
    writer.write(path.clone(), b"second".to_vec(), 0o600).unwrap();

    drop(writer);

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !tasks.live_names().is_empty() {
        assert!(std::time::Instant::now() < deadline, "the writer task never ended");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(text(&path), "second");
}

#[tokio::test]
async fn a_write_after_close_is_inline_even_from_another_thread() {
    let dir = tempfile::tempdir().unwrap();
    let (writer, _) = writer_with(Duration::from_secs(30));
    writer.write(dir.path().join("a.toml"), b"a".to_vec(), 0o600).unwrap();
    writer.close();
    let path = dir.path().join("late.toml");
    let remote = writer.clone();
    let target = path.clone();

    std::thread::spawn(move || remote.write(target, b"late".to_vec(), 0o600).unwrap()).join().unwrap();

    assert_eq!(text(&path), "late");
    wait_for_writes(&writer, 2).await;
    assert_eq!(text(&dir.path().join("a.toml")), "a");
}

#[tokio::test]
async fn a_failed_inline_drain_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let (writer, failures) = writer_with(Duration::from_secs(30));
    let first = dir.path().join("first.toml");
    writer.write(first, b"1".to_vec(), 0o600).unwrap();
    wait_for_writes(&writer, 1).await;
    writer.write(blocker.join("held.toml"), b"2".to_vec(), 0o600).unwrap();

    writer.write_pending_now();

    assert_eq!(failures.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_flush_that_cannot_finish_can_be_given_up_on() {
    use std::os::unix::ffi::OsStrExt;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("stuck.toml");
    let fifo = dir.path().join("stuck.toml.tmp");
    let c_path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let (writer, _) = writer_with(Duration::ZERO);
    writer.write(target, b"data".to_vec(), 0o600).unwrap();

    let outcome = tokio::time::timeout(Duration::from_millis(200), writer.flush()).await;

    assert!(outcome.is_err());
    let _release = std::fs::File::open(&fifo).unwrap();
}

#[test]
fn concurrent_writers_of_one_path_never_mix_their_contents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.toml");
    let long = "a".repeat(200_000).into_bytes();
    let short = "b".repeat(1_000).into_bytes();
    let handles: Vec<_> = [long.clone(), short.clone()]
        .into_iter()
        .map(|contents| {
            let path = path.clone();
            std::thread::spawn(move || {
                for _ in 0..150 {
                    crate::persist::write_atomic(&path, &contents, 0o600).unwrap();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }

    let result = std::fs::read(&path).unwrap();
    assert!(result == long || result == short, "{} bytes of mixed content", result.len());
}

#[tokio::test]
async fn a_task_that_is_aborted_writes_what_it_still_holds() {
    let dir = tempfile::tempdir().unwrap();
    let tasks = Tasks::new(|_, _, _| {});
    let writer = Writer::new(tasks.clone(), Duration::from_secs(30), |_| {});
    let path = dir.path().join("aborted.toml");
    writer.write(path.clone(), b"first".to_vec(), 0o600).unwrap();
    wait_for_writes(&writer, 1).await;
    writer.write(path.clone(), b"second".to_vec(), 0o600).unwrap();

    tasks.abort_all();
    tokio::task::yield_now().await;

    assert_eq!(text(&path), "second");
}

#[tokio::test]
async fn a_burst_of_lazy_writes_renders_only_what_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let (writer, _) = writer_with(Duration::from_secs(30));
    let path = dir.path().join("lazy.toml");
    let renders = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    for index in 0..20 {
        let counter = renders.clone();
        writer
            .write_lazy(path.clone(), 0o600, 0, move || {
                counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(format!("version {index}").into_bytes())
            })
            .unwrap();
        tokio::task::yield_now().await;
    }
    writer.flush().await;

    assert_eq!(text(&path), "version 19");
    assert!(renders.load(std::sync::atomic::Ordering::Relaxed) <= 2);
}

#[tokio::test]
async fn a_render_that_fails_is_reported_with_its_tag() {
    let dir = tempfile::tempdir().unwrap();
    let tags = Arc::new(Mutex::new(Vec::new()));
    let sink = tags.clone();
    let writer = Writer::new(Tasks::new(|_, _, _| {}), Duration::ZERO, move |failure| {
        sink.lock().unwrap().push((failure.tag, failure.message));
    });

    writer.write_lazy(dir.path().join("never.toml"), 0o600, 7, || Err("cannot render".to_string())).unwrap();
    writer.flush().await;

    assert_eq!(*tags.lock().unwrap(), vec![(7, "cannot render".to_string())]);
}

#[tokio::test]
async fn a_failed_write_carries_the_tag_it_was_queued_with() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let tags = Arc::new(Mutex::new(Vec::new()));
    let sink = tags.clone();
    let writer = Writer::new(Tasks::new(|_, _, _| {}), Duration::ZERO, move |failure| {
        sink.lock().unwrap().push(failure.tag);
    });

    writer.write_lazy(blocker.join("file.toml"), 0o600, 42, || Ok(b"data".to_vec())).unwrap();
    writer.flush().await;

    assert_eq!(*tags.lock().unwrap(), vec![42]);
}

struct WriteOnDrop(Writer, PathBuf);

impl Drop for WriteOnDrop {
    fn drop(&mut self) {
        let _ = self.0.write(self.1.clone(), b"from a dying task".to_vec(), 0o600);
    }
}

#[test]
fn a_write_issued_while_the_runtime_shuts_down_does_not_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dying.toml");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let writer = Writer::new(Tasks::new(|_, _, _| {}), Duration::ZERO, |_| {});
    let guard = WriteOnDrop(writer, path.clone());
    runtime.spawn(async move {
        let _guard = guard;
        std::future::pending::<()>().await;
    });
    runtime.block_on(tokio::task::yield_now());
    let (done, finished) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        drop(runtime);
        let _ = done.send(());
    });

    assert!(finished.recv_timeout(Duration::from_secs(5)).is_ok(), "the runtime shutdown deadlocked");
    assert_eq!(text(&path), "from a dying task");
}
