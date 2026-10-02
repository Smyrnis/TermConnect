use std::path::{Path, PathBuf};

use super::*;

fn nextcloud() -> Locator {
    Locator::new("https://cloud.example.com:443", "/remote.php/dav/files/alice/")
}

#[test]
fn the_root_of_an_empty_root_is_a_slash() {
    assert_eq!(Locator::new("http://h:80", "/").href(Path::new("/"), true), "/");
}

#[test]
fn files_are_encoded_under_the_root() {
    assert_eq!(nextcloud().href(Path::new("/docs/a b.txt"), false), "/remote.php/dav/files/alice/docs/a%20b.txt");
}

#[test]
fn collections_end_with_a_slash() {
    assert_eq!(nextcloud().href(Path::new("/docs"), true), "/remote.php/dav/files/alice/docs/");
    assert_eq!(nextcloud().href(Path::new("/"), true), "/remote.php/dav/files/alice/");
}

#[test]
fn reserved_and_non_ascii_characters_are_percent_encoded() {
    assert_eq!(
        Locator::new("http://h:80", "/").href(Path::new("/a #1 100% & ü.txt"), false),
        "/a%20%231%20100%25%20%26%20%C3%BC.txt"
    );
}

#[test]
fn parent_segments_never_escape_the_root() {
    assert_eq!(nextcloud().href(Path::new("/../../etc"), false), "/remote.php/dav/files/alice/etc");
    assert_eq!(normalize(Path::new("/a/../../b/./c")), PathBuf::from("/b/c"));
}

#[test]
fn urls_join_the_origin_and_href() {
    assert_eq!(
        nextcloud().url("/remote.php/dav/files/alice/"),
        "https://cloud.example.com:443/remote.php/dav/files/alice/"
    );
}

#[test]
fn hrefs_map_back_to_paths_under_the_root() {
    let locator = nextcloud();

    assert_eq!(
        locator.path_of_href("/remote.php/dav/files/alice/docs/a%20b.txt"),
        Some(PathBuf::from("/docs/a b.txt"))
    );
    assert_eq!(
        locator.path_of_href("https://cloud.example.com/remote.php/dav/files/alice/docs/"),
        Some(PathBuf::from("/docs"))
    );
    assert_eq!(locator.path_of_href("/remote.php/dav/files/alice/"), Some(PathBuf::from("/")));
    assert_eq!(
        locator.path_of_href("/remote.php/dav/files/alice/Tom%20&%20Jerry.txt"),
        Some(PathBuf::from("/Tom & Jerry.txt"))
    );
}

#[test]
fn hrefs_outside_the_root_are_ignored() {
    assert_eq!(nextcloud().path_of_href("/remote.php/dav/files/bob/x"), None);
}

#[test]
fn a_percent_encoded_root_is_not_encoded_twice() {
    let locator = Locator::new("https://cloud:443", "/remote.php/dav/files/john%40example.com/");

    assert_eq!(locator.href(Path::new("/"), true), "/remote.php/dav/files/john%40example.com/");
    assert_eq!(locator.path_of_href("/remote.php/dav/files/john@example.com/a.txt"), Some(PathBuf::from("/a.txt")));
}
