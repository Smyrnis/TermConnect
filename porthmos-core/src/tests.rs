use std::path::Path;

use super::*;

#[cfg(all(feature = "sftp", feature = "ftp", feature = "webdav", feature = "s3", feature = "scp"))]
#[test]
fn builtin_protocols_offer_every_protocol_in_order() {
    let protocols = builtin_protocols(&Paths::in_dir(Path::new("/tmp/t")));

    assert_eq!(
        protocols.iter().map(|protocol| protocol.id()).collect::<Vec<_>>(),
        vec!["sftp", "ftp", "webdav", "s3", "scp"]
    );
}

#[cfg(not(any(feature = "sftp", feature = "ftp", feature = "webdav", feature = "s3", feature = "scp")))]
#[test]
fn without_protocol_features_no_protocol_is_built_in() {
    assert!(builtin_protocols(&Paths::in_dir(Path::new("/tmp/t"))).is_empty());
}

#[test]
fn core_dumps_can_be_switched_off() {
    disable_core_dumps();

    let mut limit = libc::rlimit { rlim_cur: 1, rlim_max: 1 };
    assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut limit) }, 0);
    assert_eq!(limit.rlim_cur, 0);
}

#[tokio::test]
async fn a_broken_history_file_is_set_aside_with_a_warning_at_start() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    std::fs::create_dir_all(&paths.state_dir).unwrap();
    std::fs::write(paths.history_file(), "not toml [").unwrap();

    let (_core, mut events) = Core::builder().without_keyring().paths(paths.clone()).start().unwrap();

    let warning = loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv()).await;
        match event.unwrap().unwrap() {
            Event::Notice { severity: Severity::Warning, message } if message.contains("history") => break message,
            _ => {}
        }
    };
    assert!(warning.contains("history.toml.broken"), "{warning}");
    assert!(paths.history_file().with_extension("toml.broken").exists());
}

#[tokio::test]
async fn saved_history_is_loaded_at_start_and_listed_on_request() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    let mut saved = history::History::load(&paths).0;
    saved.record(history::testing::sample("kept.txt", history::HistoryResult::Done)).unwrap();

    let (core, mut events) = Core::builder().without_keyring().paths(paths).start().unwrap();
    core.send(Command::ListHistory);

    let listed = loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv()).await;
        if let Event::History(entries) = event.unwrap().unwrap() {
            break entries;
        }
    };
    assert_eq!(listed.iter().map(|entry| entry.label.as_str()).collect::<Vec<_>>(), ["kept.txt"]);
}

#[tokio::test]
async fn the_history_warning_comes_before_the_bookmark_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    std::fs::create_dir_all(&paths.state_dir).unwrap();
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.history_file(), "not toml [").unwrap();
    std::fs::write(paths.bookmarks_file(), "not toml [").unwrap();

    let (_core, mut events) = Core::builder().without_keyring().paths(paths).start().unwrap();

    let first = loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv()).await;
        if let Event::Notice { message, .. } = event.unwrap().unwrap() {
            break message;
        }
    };
    assert!(first.contains("history"), "{first}");
}
