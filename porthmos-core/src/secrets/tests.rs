use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use super::*;

fn store(backend: &Arc<TestBackend>) -> Secrets {
    Secrets::new(Some(backend.clone() as Arc<dyn SecretBackend>))
        .with_timing(Duration::from_millis(300), Duration::from_millis(30))
}

fn store_with_limit(backend: &Arc<TestBackend>, limit: Duration) -> Secrets {
    Secrets::new(Some(backend.clone() as Arc<dyn SecretBackend>)).with_timing(limit, Duration::from_millis(30))
}

fn text(secret: Option<Zeroizing<String>>) -> Option<String> {
    secret.map(|secret| secret.to_string())
}

fn waiting_reports(secrets: &Secrets) -> Arc<Mutex<Vec<bool>>> {
    let reports = Arc::new(Mutex::new(Vec::new()));
    let seen = reports.clone();
    secrets.on_waiting(move |waiting| seen.lock().unwrap().push(waiting));
    reports
}

#[test]
fn accounts_name_the_kind_and_the_owner() {
    let password = SecretKey::Profile { name: "web: prod".into(), field: SecretField::Password };
    let token = SecretKey::Profile { name: "web".into(), field: SecretField::Option("token".into()) };
    assert_eq!(password.account(), "profile:web: prod");
    assert_eq!(token.account(), "option:token:web");
    assert_eq!(SecretKey::SshHost { alias: "db 1".into() }.account(), "ssh:db 1");
}

#[test]
fn a_profile_named_like_another_profiles_option_gets_its_own_account() {
    let option = SecretKey::Profile { name: "web".into(), field: SecretField::Option("token".into()) };
    let lookalike = SecretKey::Profile { name: "web:token".into(), field: SecretField::Password };
    let tricky = SecretKey::Profile { name: "x:web".into(), field: SecretField::Option("token".into()) };

    assert_ne!(option.account(), lookalike.account());
    assert_ne!(tricky.account(), lookalike.account());
    assert_ne!(option.account(), tricky.account());
}

#[test]
fn accounts_are_read_back_into_their_keys() {
    let keys = [
        SecretKey::Profile { name: "web:token".into(), field: SecretField::Password },
        SecretKey::Profile { name: "a:b c".into(), field: SecretField::Option("token".into()) },
        SecretKey::SshHost { alias: "db:1".into() },
    ];
    for key in keys {
        assert_eq!(SecretKey::parse(&key.account()), Some(key));
    }
    assert_eq!(SecretKey::parse("porthmos:probe"), None);
    assert_eq!(SecretKey::parse("option:token"), None);
}

#[test]
fn the_service_and_probe_account_are_fixed() {
    assert_eq!(SERVICE, "porthmos");
    assert_eq!(PROBE_ACCOUNT, "porthmos:probe");
}

#[test]
fn errors_describe_the_failure_without_extra_detail() {
    assert_eq!(SecretError::TimedOut.to_string(), "timed out");
    assert_eq!(SecretError::Backend("locked".into()).to_string(), "locked");
}

#[tokio::test]
async fn lookup_reads_the_keyring_only_when_a_secret_is_saved_and_caches_it() {
    let backend = Arc::new(TestBackend::new());
    backend.put("profile:web", "pw");
    let secrets = store(&backend);

    assert_eq!(text(secrets.lookup("profile:web", false).await), None);
    assert!(backend.calls().is_empty());
    assert_eq!(text(secrets.lookup("profile:web", true).await).as_deref(), Some("pw"));
    assert_eq!(text(secrets.lookup("profile:web", true).await).as_deref(), Some("pw"));
    assert_eq!(text(secrets.lookup("profile:web", false).await).as_deref(), Some("pw"));
    assert_eq!(backend.calls(), vec!["get profile:web"]);
}

#[tokio::test]
async fn a_missing_keyring_entry_is_not_cached_so_a_later_save_is_seen() {
    let backend = Arc::new(TestBackend::new());
    let secrets = store(&backend);

    assert_eq!(text(secrets.lookup("profile:web", true).await), None);
    backend.put("profile:web", "later");
    assert_eq!(text(secrets.lookup("profile:web", true).await).as_deref(), Some("later"));
    assert_eq!(backend.calls(), vec!["get profile:web", "get profile:web"]);
}

#[test]
fn uncache_drops_only_the_run_copy() {
    let backend = Arc::new(TestBackend::new());
    backend.put("profile:web", "pw");
    let secrets = store(&backend);
    secrets.remember("profile:web", "pw");
    secrets.remember("profile:other", "x");

    secrets.uncache("profile:web");

    assert!(secrets.cached("profile:web").is_none());
    assert_eq!(text(secrets.cached("profile:other")).as_deref(), Some("x"));
    assert_eq!(backend.stored("profile:web").as_deref(), Some("pw"));
    assert!(backend.calls().is_empty());
}

#[tokio::test]
async fn remember_only_caches_and_save_writes_both() {
    let backend = Arc::new(TestBackend::new());
    let secrets = store(&backend);

    secrets.remember("ssh:a", "one");
    assert_eq!(text(secrets.cached("ssh:a")).as_deref(), Some("one"));
    assert!(backend.stored("ssh:a").is_none());
    assert!(backend.calls().is_empty());

    assert!(secrets.save("ssh:b", "two").await.unwrap());
    assert_eq!(backend.stored("ssh:b").as_deref(), Some("two"));
    assert_eq!(text(secrets.cached("ssh:b")).as_deref(), Some("two"));
    assert_eq!(backend.calls(), vec!["set ssh:b"]);
}

#[tokio::test]
async fn saving_again_replaces_both_layers() {
    let backend = Arc::new(TestBackend::new());
    let secrets = store(&backend);

    secrets.save("profile:web", "old").await.unwrap();
    secrets.save("profile:web", "new").await.unwrap();

    assert_eq!(backend.stored("profile:web").as_deref(), Some("new"));
    assert_eq!(text(secrets.cached("profile:web")).as_deref(), Some("new"));
}

#[tokio::test]
async fn accounts_do_not_leak_into_each_other() {
    let backend = Arc::new(TestBackend::new());
    let secrets = store(&backend);

    secrets.save("profile:web", "a").await.unwrap();
    secrets.save("option:token:web", "b").await.unwrap();
    secrets.save("ssh:web", "c").await.unwrap();

    assert_eq!(text(secrets.cached("profile:web")).as_deref(), Some("a"));
    assert_eq!(text(secrets.cached("option:token:web")).as_deref(), Some("b"));
    assert_eq!(text(secrets.cached("ssh:web")).as_deref(), Some("c"));
    secrets.forget("profile:web", true).await.unwrap();
    assert_eq!(backend.stored("option:token:web").as_deref(), Some("b"));
    assert_eq!(backend.stored("ssh:web").as_deref(), Some("c"));
}

#[tokio::test]
async fn a_failing_read_counts_as_not_saved_and_is_not_cached() {
    let backend = Arc::new(TestBackend::new());
    backend.put("profile:web", "pw");
    backend.fail_reads();
    let secrets = store(&backend);

    assert_eq!(text(secrets.lookup("profile:web", true).await), None);
    assert!(secrets.cached("profile:web").is_none());
}

#[tokio::test]
async fn a_failing_write_is_an_error_but_the_run_keeps_the_secret() {
    let backend = Arc::new(TestBackend::new());
    backend.fail_writes();
    let secrets = store(&backend);

    assert_eq!(secrets.save("profile:web", "pw").await, Err(SecretError::Backend("write failed".into())));
    assert_eq!(text(secrets.cached("profile:web")).as_deref(), Some("pw"));
    assert!(backend.stored("profile:web").is_none());
}

#[tokio::test]
async fn a_failing_delete_is_an_error_but_the_run_forgets_the_secret() {
    let backend = Arc::new(TestBackend::new());
    backend.put("profile:web", "pw");
    let secrets = store(&backend);
    secrets.remember("profile:web", "pw");
    backend.fail_writes();

    assert!(secrets.forget("profile:web", true).await.is_err());
    assert!(secrets.cached("profile:web").is_none());
    assert_eq!(backend.stored("profile:web").as_deref(), Some("pw"));
}

#[tokio::test]
async fn a_slow_keyring_times_out_on_every_operation() {
    let backend = Arc::new(TestBackend::new());
    backend.put("profile:web", "pw");
    backend.block_for(Duration::from_millis(600));
    let secrets = store(&backend);

    assert_eq!(text(secrets.lookup("profile:web", true).await), None);
    assert_eq!(secrets.save("profile:x", "y").await, Err(SecretError::TimedOut));
    assert_eq!(text(secrets.cached("profile:x")).as_deref(), Some("y"));
    assert_eq!(secrets.forget("profile:web", true).await, Err(SecretError::TimedOut));
    assert_eq!(secrets.rename("profile:a", "profile:b", true).await, Err(SecretError::TimedOut));
}

#[tokio::test]
async fn a_slow_keyring_reports_waiting_then_done() {
    let backend = Arc::new(TestBackend::new());
    backend.put("profile:web", "pw");
    backend.block_for(Duration::from_millis(100));
    let secrets = store(&backend);
    let reports = waiting_reports(&secrets);

    assert_eq!(text(secrets.lookup("profile:web", true).await).as_deref(), Some("pw"));
    assert_eq!(*reports.lock().unwrap(), vec![true, false]);
}

#[tokio::test]
async fn a_timed_out_call_still_reports_done() {
    let backend = Arc::new(TestBackend::new());
    backend.block_for(Duration::from_millis(600));
    let secrets = store(&backend);
    let reports = waiting_reports(&secrets);

    assert_eq!(secrets.save("profile:x", "y").await, Err(SecretError::TimedOut));
    assert_eq!(*reports.lock().unwrap(), vec![true, false]);
}

#[tokio::test]
async fn a_fast_keyring_reports_no_waiting() {
    let backend = Arc::new(TestBackend::new());
    let secrets = store(&backend);
    let reports = waiting_reports(&secrets);

    secrets.save("profile:x", "y").await.unwrap();

    assert!(reports.lock().unwrap().is_empty());
}

#[tokio::test]
async fn without_a_keyring_everything_stays_in_the_run_cache() {
    let secrets = Secrets::new(None);

    assert!(!secrets.available());
    assert!(!secrets.probe().await);
    assert_eq!(secrets.save("profile:web", "pw").await, Ok(false));
    assert_eq!(text(secrets.lookup("profile:web", true).await).as_deref(), Some("pw"));
    secrets.rename("profile:web", "profile:site", true).await.unwrap();
    assert_eq!(text(secrets.cached("profile:site")).as_deref(), Some("pw"));
    secrets.forget("profile:site", true).await.unwrap();
    assert!(secrets.cached("profile:site").is_none());
    assert_eq!(text(secrets.lookup("profile:site", true).await), None);
}

#[tokio::test]
async fn after_a_failed_probe_the_keyring_is_never_called_again() {
    let backend = Arc::new(TestBackend::new());
    backend.fail_reads();
    let secrets = store(&backend);
    assert!(!secrets.probe().await);

    assert_eq!(secrets.save("profile:web", "pw").await, Ok(false));
    assert_eq!(text(secrets.lookup("profile:other", true).await), None);
    secrets.forget("profile:web", true).await.unwrap();
    secrets.rename("profile:a", "profile:b", true).await.unwrap();

    assert_eq!(backend.calls(), vec!["get porthmos:probe"]);
}

#[tokio::test]
async fn rename_moves_both_layers_and_forget_removes_both() {
    let backend = Arc::new(TestBackend::new());
    backend.put("profile:old", "pw");
    let secrets = store(&backend);
    secrets.remember("profile:old", "pw");

    secrets.rename("profile:old", "profile:new", true).await.unwrap();
    assert!(backend.stored("profile:old").is_none());
    assert_eq!(backend.stored("profile:new").as_deref(), Some("pw"));
    assert!(secrets.cached("profile:old").is_none());
    assert_eq!(text(secrets.cached("profile:new")).as_deref(), Some("pw"));
    assert_eq!(backend.calls(), vec!["get profile:old", "set profile:new", "delete profile:old"]);

    secrets.forget("profile:new", true).await.unwrap();
    assert!(backend.stored("profile:new").is_none());
    assert!(secrets.cached("profile:new").is_none());
}

#[tokio::test]
async fn a_rename_that_cannot_write_the_new_entry_keeps_the_old_one() {
    let backend = Arc::new(TestBackend::new());
    backend.put("profile:old", "pw");
    backend.fail_writes();
    let secrets = store(&backend);

    assert!(secrets.rename("profile:old", "profile:new", true).await.is_err());

    assert_eq!(backend.stored("profile:old").as_deref(), Some("pw"));
    assert!(backend.stored("profile:new").is_none());
    assert_eq!(backend.calls(), vec!["get profile:old", "set profile:new"]);
}

#[tokio::test]
async fn renaming_a_secret_that_is_not_in_the_keyring_writes_nothing() {
    let backend = Arc::new(TestBackend::new());
    let secrets = store(&backend);

    secrets.rename("profile:old", "profile:new", true).await.unwrap();

    assert_eq!(backend.calls(), vec!["get profile:old"]);
}

#[tokio::test]
async fn forgetting_without_a_saved_secret_never_calls_the_keyring() {
    let backend = Arc::new(TestBackend::new());
    let secrets = store(&backend);
    secrets.remember("profile:web", "pw");

    secrets.forget("profile:web", false).await.unwrap();
    secrets.rename("profile:a", "profile:b", false).await.unwrap();

    assert!(backend.calls().is_empty());
    assert!(secrets.cached("profile:web").is_none());
}

#[tokio::test]
async fn the_probe_decides_availability() {
    let working = Arc::new(TestBackend::new());
    let secrets = store(&working);
    assert!(secrets.probe().await);
    assert!(secrets.available());
    assert_eq!(working.calls(), vec!["get porthmos:probe"]);

    let broken = Arc::new(TestBackend::new());
    broken.fail_reads();
    let secrets = store(&broken);
    assert!(!secrets.probe().await);
    assert!(!secrets.available());
}

#[tokio::test]
async fn a_probe_that_times_out_means_no_keyring() {
    let backend = Arc::new(TestBackend::new());
    backend.block_for(Duration::from_millis(600));
    let secrets = store(&backend);

    assert!(!secrets.probe().await);
    assert!(!secrets.available());
}

#[tokio::test]
async fn clones_share_one_cache_and_one_availability() {
    let backend = Arc::new(TestBackend::new());
    backend.fail_reads();
    let secrets = store(&backend);
    let other = secrets.clone();

    other.remember("profile:web", "pw");
    assert!(!other.probe().await);

    assert_eq!(text(secrets.cached("profile:web")).as_deref(), Some("pw"));
    assert!(!secrets.available());
}

#[tokio::test]
async fn overlapping_slow_calls_report_waiting_once_until_the_last_ends() {
    let backend = Arc::new(TestBackend::new());
    backend.put("profile:a", "1");
    backend.put("profile:b", "2");
    backend.block_for(Duration::from_millis(80));
    let secrets = store_with_limit(&backend, Duration::from_secs(2));
    let reports = waiting_reports(&secrets);

    let (a, b) = tokio::join!(secrets.lookup("profile:a", true), secrets.lookup("profile:b", true));

    assert_eq!((text(a).as_deref(), text(b).as_deref()), (Some("1"), Some("2")));
    assert_eq!(*reports.lock().unwrap(), vec![true, false]);
}

#[tokio::test]
async fn a_timed_out_write_lands_before_a_later_delete() {
    let backend = Arc::new(TestBackend::new());
    backend.block_next(Duration::from_millis(500));
    let secrets = store_with_limit(&backend, Duration::from_millis(300));

    assert_eq!(secrets.save("profile:web", "pw").await, Err(SecretError::TimedOut));
    secrets.forget("profile:web", true).await.unwrap();

    assert!(backend.stored("profile:web").is_none());
    assert_eq!(backend.calls(), vec!["set profile:web", "delete profile:web"]);
}

#[test]
fn cached_secrets_can_be_listed_and_moved() {
    let secrets = Secrets::new(None);
    secrets.remember("profile:web", "pw");
    secrets.remember("option:token:web", "tok");

    let mut accounts = secrets.cached_accounts();
    accounts.sort();
    assert_eq!(accounts, vec!["option:token:web", "profile:web"]);

    secrets.move_cached("profile:web", "profile:site");
    secrets.move_cached("profile:missing", "profile:other");

    assert!(secrets.cached("profile:web").is_none());
    assert_eq!(text(secrets.cached("profile:site")).as_deref(), Some("pw"));
    assert!(secrets.cached("profile:other").is_none());
}

#[tokio::test]
async fn keyring_only_calls_leave_the_run_cache_alone() {
    let backend = Arc::new(TestBackend::new());
    let secrets = store(&backend);
    secrets.remember("profile:new", "cached-new");

    assert!(secrets.write_keyring("profile:old", "pw").await.unwrap());
    assert!(secrets.cached("profile:old").is_none());

    secrets.move_keyring("profile:old", "profile:site").await.unwrap();
    assert_eq!(backend.stored("profile:site").as_deref(), Some("pw"));
    assert!(backend.stored("profile:old").is_none());
    assert!(secrets.cached("profile:site").is_none());

    secrets.erase_keyring("profile:site").await.unwrap();
    assert!(backend.stored("profile:site").is_none());
    assert_eq!(text(secrets.cached("profile:new")).as_deref(), Some("cached-new"));
    assert_eq!(
        backend.calls(),
        vec!["set profile:old", "get profile:old", "set profile:site", "delete profile:old", "delete profile:site"]
    );
}

#[tokio::test]
async fn keyring_only_calls_without_a_keyring_do_nothing() {
    let secrets = Secrets::new(None);

    assert_eq!(secrets.write_keyring("profile:web", "pw").await, Ok(false));
    assert_eq!(secrets.move_keyring("profile:a", "profile:b").await, Ok(()));
    assert_eq!(secrets.erase_keyring("profile:a").await, Ok(()));
    assert!(secrets.cached("profile:web").is_none());
}

#[cfg(feature = "keyring")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_native_keyring_can_be_set_up_inside_the_runtime() {
    let secrets = Secrets::native();

    assert!(secrets.available());
    assert!(secrets.cached("profile:web").is_none());
}
