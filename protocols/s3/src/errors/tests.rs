use std::path::Path;

use porthmos_vfs::ErrorKind;

use super::*;

fn mapped(status: u16, code: &str, message: &str) -> (ErrorKind, String) {
    let error = S3Error { status, code: code.to_string(), message: message.to_string(), region: None };
    let mapped = to_protocol(&error, Path::new("/b/a.txt"));
    (mapped.kind(), mapped.to_string())
}

#[test]
fn s3_codes_map_to_kinds_and_messages() {
    for code in ["NoSuchKey", "NoSuchBucket", "NoSuchUpload"] {
        assert_eq!(mapped(404, code, "x"), (ErrorKind::NotFound, "/b/a.txt not found".to_string()));
    }
    assert_eq!(mapped(404, "", ""), (ErrorKind::NotFound, "/b/a.txt not found".to_string()));
    assert_eq!(
        mapped(403, "AccessDenied", "Access Denied"),
        (ErrorKind::PermissionDenied, "/b/a.txt: permission denied".to_string())
    );
    assert_eq!(mapped(403, "", ""), (ErrorKind::PermissionDenied, "/b/a.txt: permission denied".to_string()));
    assert_eq!(
        mapped(503, "SlowDown", "Reduce"),
        (ErrorKind::Other, "the server is busy (SlowDown) \u{2014} try again".to_string())
    );
    assert_eq!(mapped(503, "", ""), (ErrorKind::Other, "the server is busy (SlowDown) \u{2014} try again".to_string()));
    assert_eq!(
        mapped(400, "EntityTooLarge", "big"),
        (ErrorKind::Other, "/b/a.txt is too large for this server".to_string())
    );
    assert_eq!(mapped(409, "BucketNotEmpty", "not empty"), (ErrorKind::Other, "BucketNotEmpty: not empty".to_string()));
    assert_eq!(mapped(500, "", ""), (ErrorKind::Other, "500 Internal Server Error".to_string()));
}

#[tokio::test]
async fn request_errors_keep_the_cause_and_drop_the_url() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let err = reqwest::Client::new().get(format!("http://127.0.0.1:{port}/secret-key")).send().await.unwrap_err();

    let error = Failure::Request(err).into_error(ErrorKind::Connect);

    assert_eq!(error.kind(), ErrorKind::Connect);
    assert!(!error.to_string().contains("secret-key"), "{error}");
}

#[test]
fn timeouts_have_one_message() {
    let error = Failure::TimedOut.into_error(ErrorKind::Other);

    assert_eq!(error.to_string(), "the server did not respond in time");
}
