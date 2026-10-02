use std::path::Path;

use porthmos_vfs::ErrorKind;
use reqwest::{
    StatusCode,
    header::{HeaderMap, HeaderValue, LOCATION},
};

use super::*;

fn mapped(code: u16) -> (ErrorKind, String) {
    let error = status_error(StatusCode::from_u16(code).unwrap(), &HeaderMap::new(), Path::new("/docs/a.txt"));
    (error.kind(), error.to_string())
}

#[test]
fn statuses_map_to_kinds_and_messages() {
    assert_eq!(mapped(401), (ErrorKind::Auth, "authentication expired".to_string()));
    assert_eq!(mapped(403), (ErrorKind::PermissionDenied, "/docs/a.txt: permission denied".to_string()));
    assert_eq!(mapped(404), (ErrorKind::NotFound, "/docs/a.txt not found".to_string()));
    assert_eq!(mapped(409), (ErrorKind::NotFound, "/docs/a.txt not found".to_string()));
    assert_eq!(mapped(423), (ErrorKind::Other, "/docs/a.txt is locked".to_string()));
    assert_eq!(mapped(507), (ErrorKind::Other, "insufficient storage on the server".to_string()));
    assert_eq!(mapped(500), (ErrorKind::Other, "500 Internal Server Error".to_string()));
}

#[test]
fn redirects_name_the_location() {
    let mut headers = HeaderMap::new();
    headers.insert(LOCATION, HeaderValue::from_static("https://nas.local/dav/"));

    let error = status_error(StatusCode::MOVED_PERMANENTLY, &headers, Path::new("/"));

    assert_eq!(error.kind(), ErrorKind::Connect);
    assert_eq!(
        error.to_string(),
        "the server redirected to https://nas.local/dav/ \u{2014} check Security and Root path"
    );
}

#[tokio::test]
async fn request_errors_keep_the_cause_and_drop_the_url() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let err = reqwest::Client::new().get(format!("http://127.0.0.1:{port}/secret-path")).send().await.unwrap_err();

    let error = request_error(ErrorKind::Connect, err);

    assert_eq!(error.kind(), ErrorKind::Connect);
    assert!(!error.to_string().contains("secret-path"), "{error}");
    assert!(error.to_string().to_ascii_lowercase().contains("refused"), "{error}");
}
