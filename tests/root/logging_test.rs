use std::fs;
use std::io::Write;

use super::*;

#[test]
fn open_writer_at_creates_parent_directories_and_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("termconnect.log");

    open_writer_at(&path).unwrap();

    assert!(path.exists());
}

#[test]
fn open_writer_at_appends_rather_than_truncating_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("termconnect.log");
    {
        let mut file = open_writer_at(&path).unwrap();
        writeln!(file, "first").unwrap();
    }
    {
        let mut file = open_writer_at(&path).unwrap();
        writeln!(file, "second").unwrap();
    }

    let contents = fs::read_to_string(&path).unwrap();
    assert_eq!(contents, "first\nsecond\n");
}
