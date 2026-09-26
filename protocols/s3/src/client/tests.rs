use std::time::Duration;

use reqwest::Method;

use super::*;

#[test]
fn reads_wait_four_timeouts_for_a_reply() {
    assert_eq!(reply_limit(&Method::GET, 0, Duration::from_secs(30)), Duration::from_secs(120));
}

#[test]
fn writes_wait_longer_the_more_they_send() {
    let timeout = Duration::from_secs(30);

    assert_eq!(reply_limit(&Method::DELETE, 0, timeout), Duration::from_secs(600));
    assert_eq!(reply_limit(&Method::PUT, 16 * 1024 * 1024, timeout), Duration::from_secs(600 + 1024));
}

#[test]
fn only_a_real_new_region_is_a_hint() {
    assert_eq!(useful_hint(Some("eu-west-1".to_string()), "us-east-1"), Some("eu-west-1".to_string()));
    assert_eq!(useful_hint(Some(String::new()), "us-east-1"), None);
    assert_eq!(useful_hint(Some("us-east-1".to_string()), "us-east-1"), None);
    assert_eq!(useful_hint(None, "us-east-1"), None);
}
