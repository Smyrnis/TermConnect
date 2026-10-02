use super::*;

#[test]
fn a_fresh_upload_stores_from_the_start() {
    assert_eq!(first_write_start(0), WriteStart::Store);
}

#[test]
fn a_resumed_upload_tries_restart_then_append_then_starts_over() {
    assert_eq!(first_write_start(6), WriteStart::RestartStore);
    assert_eq!(fallback(WriteStart::RestartStore), Some(WriteStart::Append));
    assert_eq!(fallback(WriteStart::Append), Some(WriteStart::Store));
    assert_eq!(fallback(WriteStart::Store), None);
}

#[test]
fn only_storing_from_the_start_resets_the_offset() {
    assert_eq!(WriteStart::RestartStore.offset(6), 6);
    assert_eq!(WriteStart::Append.offset(6), 6);
    assert_eq!(WriteStart::Store.offset(6), 0);
}

#[test]
fn a_tls_data_stream_closed_without_close_notify_counts_as_the_end_of_the_data() {
    assert!(ends_data(&io::Error::from(io::ErrorKind::UnexpectedEof)));
    assert!(!ends_data(&io::Error::from(io::ErrorKind::ConnectionReset)));
}
