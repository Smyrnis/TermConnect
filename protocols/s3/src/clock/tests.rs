use super::*;

#[test]
fn amz_dates_are_utc_calendar_values() {
    assert_eq!(amz_date(1_369_353_600), ("20130524".to_string(), "20130524T000000Z".to_string()));
    assert_eq!(amz_date(1_790_430_698), ("20260926".to_string(), "20260926T135138Z".to_string()));
    assert_eq!(amz_date(951_782_400), ("20000229".to_string(), "20000229T000000Z".to_string()));
}

#[test]
fn iso_8601_timestamps_parse_to_unix_seconds() {
    assert_eq!(parse_iso8601("2026-09-26T13:51:38.947Z"), Some(1_790_430_698));
    assert_eq!(parse_iso8601("2013-05-24T00:00:00Z"), Some(1_369_353_600));
    assert_eq!(parse_iso8601("not a date"), None);
    assert_eq!(parse_iso8601("2026-13-01T00:00:00Z"), None);
}

#[test]
fn now_is_after_2026() {
    assert!(now() > 1_767_225_600);
}

#[test]
fn server_times_are_moved_by_the_measured_skew() {
    assert_eq!(skew(1_000, Some("Thu, 01 Jan 1970 00:15:00 GMT")), 100);
    assert_eq!(skew(1_000, Some("garbage")), 0);
    assert_eq!(skew(1_000, None), 0);
    assert_eq!(adjust(Some(500), 100), Some(600));
    assert_eq!(adjust(Some(50), -100), Some(0));
    assert_eq!(adjust(None, 100), None);
}
