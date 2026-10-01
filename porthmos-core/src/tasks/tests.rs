use std::{
    sync::{Arc, Mutex, atomic::Ordering},
    time::{Duration, Instant},
};

use tokio::sync::oneshot;

use super::*;

type PanicLog = Arc<Mutex<Vec<(&'static str, Scope, String)>>>;

fn tasks_with_panics() -> (Tasks, PanicLog) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let tasks = Tasks::new(move |name, scope, message| sink.lock().unwrap().push((name, scope, message)));
    (tasks, seen)
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !condition() {
        assert!(Instant::now() < deadline, "condition not reached in time");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn until_cancelled(cancel: Arc<std::sync::atomic::AtomicBool>, done: oneshot::Sender<()>) {
    while !cancel.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let _ = done.send(());
}

#[tokio::test]
async fn a_finished_task_leaves_the_live_list() {
    let (tasks, _) = tasks_with_panics();
    let (sender, receiver) = oneshot::channel();

    tasks.spawn("quick", Scope::Background, move |_| async move {
        let _ = sender.send(());
    });
    receiver.await.unwrap();

    wait_until(|| tasks.live_names().is_empty()).await;
}

#[tokio::test]
async fn running_tasks_are_listed_by_name() {
    let (tasks, _) = tasks_with_panics();
    let (hold, held) = oneshot::channel::<()>();
    let (other_hold, other_held) = oneshot::channel::<()>();

    tasks.spawn("alpha", Scope::Background, move |_| async move {
        let _ = held.await;
    });
    tasks.spawn("beta", Scope::Session(1), move |_| async move {
        let _ = other_held.await;
    });

    assert_eq!(tasks.live_names(), vec!["alpha", "beta"]);
    drop((hold, other_hold));
}

#[tokio::test]
async fn the_flag_a_task_receives_is_the_flag_spawn_returns() {
    let (tasks, _) = tasks_with_panics();
    let (sender, receiver) = oneshot::channel();

    let returned = tasks.spawn("probe", Scope::Background, move |cancel| async move {
        let _ = sender.send(cancel);
    });

    let received = receiver.await.unwrap();
    assert!(Arc::ptr_eq(&returned, &received));
}

#[tokio::test]
async fn cancelling_a_scope_stops_only_that_scope() {
    let (tasks, _) = tasks_with_panics();
    let (first_done, first_finished) = oneshot::channel();
    let (second_done, second_finished) = oneshot::channel();
    let first = tasks.spawn("first", Scope::Transfer(1), move |cancel| until_cancelled(cancel, first_done));
    let second = tasks.spawn("second", Scope::Transfer(2), move |cancel| until_cancelled(cancel, second_done));

    tasks.cancel(Scope::Transfer(1));
    first_finished.await.unwrap();

    assert!(first.load(Ordering::Relaxed));
    assert!(!second.load(Ordering::Relaxed));
    tasks.cancel_all();
    second_finished.await.unwrap();
}

#[tokio::test]
async fn a_cancel_is_remembered_until_it_is_taken_even_with_no_live_task() {
    let (tasks, _) = tasks_with_panics();

    tasks.cancel(Scope::Edit(4));

    assert!(tasks.take_cancelled(Scope::Edit(4)));
    assert!(!tasks.take_cancelled(Scope::Edit(4)));
    assert!(!tasks.take_cancelled(Scope::Edit(5)));
}

#[tokio::test]
async fn forgetting_a_scope_drops_its_cancel_mark() {
    let (tasks, _) = tasks_with_panics();
    tasks.cancel(Scope::Planning(2));

    tasks.forget(Scope::Planning(2));

    assert!(!tasks.take_cancelled(Scope::Planning(2)));
}

#[tokio::test]
async fn a_panicking_task_is_reported_with_its_name_and_does_not_affect_others() {
    let (tasks, seen) = tasks_with_panics();
    let (done, finished) = oneshot::channel();
    let survivor = tasks.spawn("survivor", Scope::Background, move |cancel| until_cancelled(cancel, done));

    tasks.spawn("crasher", Scope::Background, |_| async move {
        panic!("boom from a task");
    });
    wait_until(|| !seen.lock().unwrap().is_empty()).await;

    let reports = seen.lock().unwrap().clone();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].0, "crasher");
    assert!(reports[0].2.contains("boom from a task"), "{}", reports[0].2);
    assert_eq!(tasks.live_names(), vec!["survivor"]);
    survivor.store(true, Ordering::Relaxed);
    finished.await.unwrap();
}

#[tokio::test]
async fn a_panic_with_an_owned_message_is_reported_too() {
    let (tasks, seen) = tasks_with_panics();

    tasks.spawn("owned", Scope::Background, |_| async move {
        let message = format!("code {}", 7);
        panic!("{message}");
    });
    wait_until(|| !seen.lock().unwrap().is_empty()).await;

    assert!(seen.lock().unwrap()[0].2.contains("code 7"));
    assert_eq!(seen.lock().unwrap()[0].1, Scope::Background);
}

#[tokio::test]
async fn shutdown_waits_for_tasks_that_finish_within_the_grace() {
    let (tasks, _) = tasks_with_panics();
    tasks.spawn("slowish", Scope::Background, |_| async move {
        tokio::time::sleep(Duration::from_millis(60)).await;
    });
    let start = Instant::now();

    tasks.shutdown(Duration::from_secs(2)).await;

    assert!(start.elapsed() >= Duration::from_millis(50));
    assert!(start.elapsed() < Duration::from_millis(1500));
    assert!(tasks.live_names().is_empty());
}

#[tokio::test]
async fn shutdown_aborts_a_task_that_ignores_cancellation_after_the_grace() {
    let (tasks, _) = tasks_with_panics();
    tasks.spawn("stubborn", Scope::Background, |_| async move {
        std::future::pending::<()>().await;
    });
    let start = Instant::now();

    tasks.shutdown(Duration::from_millis(80)).await;

    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(70), "{elapsed:?}");
    assert!(elapsed < Duration::from_millis(1500), "{elapsed:?}");
    assert!(tasks.live_names().is_empty());
}

#[tokio::test]
async fn shutdown_with_nothing_running_returns_at_once_and_can_repeat() {
    let (tasks, _) = tasks_with_panics();
    let start = Instant::now();

    tasks.shutdown(Duration::from_secs(5)).await;
    tasks.shutdown(Duration::from_secs(5)).await;

    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn no_task_is_spawned_outside_the_supervisor() {
    fn visit(dir: &std::path::Path, offenders: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, offenders);
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let allowed = path.ends_with("tasks/mod.rs") || path.ends_with("src/lib.rs");
            if !name.ends_with(".rs") || name == "tests.rs" || name == "testing.rs" || allowed {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for (number, line) in text.lines().enumerate() {
                let spawns = line.contains("tokio::spawn(")
                    || line.contains("task::spawn(")
                    || line.contains("use tokio::spawn;")
                    || line.contains("use tokio::task::spawn;")
                    || line.contains("use tokio::{spawn");
                if spawns {
                    offenders.push(format!("{}:{}", path.display(), number + 1));
                }
            }
        }
    }

    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    visit(&source, &mut offenders);

    assert!(offenders.is_empty(), "spawned outside Tasks: {offenders:?}");
}

#[tokio::test]
async fn a_panic_is_reported_with_the_scope_of_its_task() {
    let (tasks, seen) = tasks_with_panics();

    tasks.spawn("scoped", Scope::Transfer(9), |_| async move {
        panic!("scoped boom");
    });
    wait_until(|| !seen.lock().unwrap().is_empty()).await;

    assert_eq!(seen.lock().unwrap()[0].1, Scope::Transfer(9));
}

#[tokio::test]
async fn code_running_in_a_task_knows_it_is_supervised_and_other_code_does_not() {
    let (tasks, _) = tasks_with_panics();
    let (sender, receiver) = oneshot::channel();

    tasks.spawn("probe", Scope::Background, move |_| async move {
        let before = in_supervised_task();
        tokio::task::yield_now().await;
        let _ = sender.send((before, in_supervised_task()));
    });

    assert_eq!(receiver.await.unwrap(), (true, true));
    assert!(!in_supervised_task());
}

#[tokio::test]
async fn aborting_everything_stops_tasks_that_ignore_cancellation_and_empties_the_list() {
    let (tasks, _) = tasks_with_panics();
    tasks.spawn("stubborn", Scope::Background, |_| async move {
        std::future::pending::<()>().await;
    });

    tasks.abort_all();

    assert!(tasks.live_names().is_empty());
}

#[tokio::test]
async fn waiting_for_a_name_returns_when_those_tasks_have_finished() {
    let (tasks, _) = tasks_with_panics();
    tasks.spawn("upload", Scope::Edit(1), |_| async move {
        tokio::time::sleep(Duration::from_millis(60)).await;
    });
    let start = Instant::now();

    let finished = tasks.wait_for_name("upload", Duration::from_secs(2)).await;

    assert!(finished);
    assert!(start.elapsed() >= Duration::from_millis(50));
    assert!(tasks.live_names().is_empty());
}

#[tokio::test]
async fn waiting_for_a_name_gives_up_after_the_timeout_and_leaves_the_task_running() {
    let (tasks, _) = tasks_with_panics();
    let (hold, held) = oneshot::channel::<()>();
    tasks.spawn("upload", Scope::Edit(1), move |_| async move {
        let _ = held.await;
    });

    let finished = tasks.wait_for_name("upload", Duration::from_millis(60)).await;

    assert!(!finished);
    assert_eq!(tasks.live_names(), vec!["upload"]);
    drop(hold);
}

#[tokio::test]
async fn waiting_for_a_name_ignores_tasks_with_other_names() {
    let (tasks, _) = tasks_with_panics();
    let (hold, held) = oneshot::channel::<()>();
    tasks.spawn("other", Scope::Background, move |_| async move {
        let _ = held.await;
    });
    let start = Instant::now();

    let finished = tasks.wait_for_name("upload", Duration::from_secs(5)).await;

    assert!(finished);
    assert!(start.elapsed() < Duration::from_secs(1));
    drop(hold);
}
