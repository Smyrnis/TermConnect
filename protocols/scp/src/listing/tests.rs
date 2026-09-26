use porthmos_vfs::FileKind;

use super::*;

const NOW: u64 = 1_790_430_698;
const SEPT_26_17_22: u64 = 1_790_443_320;

fn line(text: &str) -> Line {
    parse_line(text, NOW).unwrap()
}

#[test]
fn a_gnu_file_line_gives_name_size_time_and_mode() {
    let parsed = line("-rw-rw-r--  1 1000 1000    5 Sep 26 17:22 a b.txt");

    assert_eq!(parsed.name, "a b.txt");
    assert_eq!(
        (parsed.metadata.kind, parsed.metadata.size, parsed.metadata.modified, parsed.metadata.permissions),
        (FileKind::File, 5, Some(SEPT_26_17_22), Some(0o664))
    );
}

#[test]
fn a_busybox_folder_line_is_a_folder() {
    let parsed = line("drwxr-xr-x    2 0        0             60 Sep 26 17:22 etc");

    assert_eq!(
        (parsed.name.as_str(), parsed.metadata.kind, parsed.metadata.permissions),
        ("etc", FileKind::Dir, Some(0o755))
    );
}

#[test]
fn a_device_has_no_size() {
    let parsed = line("crw-rw-rw-  1 0 0 1, 3 Sep 26 17:22 null");

    assert_eq!((parsed.name.as_str(), parsed.metadata.size), ("null", 0));
}

#[test]
fn names_keep_leading_spaces_quotes_and_unicode() {
    assert_eq!(line("-rw-r--r-- 1 0 0 1 Sep 26 17:22  lead").name, " lead");
    assert_eq!(line("-rw-r--r-- 1 0 0 1 Sep 26 17:22 it's \"q\" ü").name, "it's \"q\" ü");
}

#[test]
fn an_old_date_shows_the_year_and_means_midnight() {
    assert_eq!(line("-rw-r--r-- 1 0 0 1 Jan  1  2025 old").metadata.modified, Some(1_735_689_600));
}

#[test]
fn a_time_in_the_future_belongs_to_last_year() {
    assert_eq!(line("-rw-r--r-- 1 0 0 1 Dec 31 23:59 late").metadata.modified, Some(1_767_225_540));
}

#[test]
fn special_mode_bits_are_kept() {
    assert_eq!(line("-rwsr-xr-x 1 0 0 1 Sep 26 17:22 su").metadata.permissions, Some(0o4755));
    assert_eq!(line("drwxrwxrwt 1 0 0 1 Sep 26 17:22 tmp").metadata.permissions, Some(0o1777));
    assert_eq!(line("-rwSr--r-- 1 0 0 1 Sep 26 17:22 odd").metadata.permissions, Some(0o4644));
}

#[test]
fn listings_skip_totals_dots_and_garbage() {
    let output = "total 8\ndrwxr-xr-x 2 0 0 60 Sep 26 17:22 .\ndrwxr-xr-x 9 0 0 60 Sep 26 17:22 ..\n-rw-r--r-- 1 0 0 5 Sep 26 17:22 keep\nnot a listing line\n";

    let names: Vec<String> = parse(output, NOW).into_iter().map(|line| line.name).collect();

    assert_eq!(names, vec!["keep".to_string()]);
}

#[test]
fn a_line_whose_date_cannot_be_read_is_skipped() {
    assert_eq!(parse_line("-rw-rw-r-- 1 1000 1000 0 2026-09-26 18:05 a b.txt", NOW), None);
}
