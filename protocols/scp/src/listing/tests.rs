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

#[test]
fn an_epoch_time_style_gives_an_exact_modification_time() {
    let parsed = line("-rw-r--r--  1 1000 1000     5 1400000000 report final.txt");

    assert_eq!(parsed.name, "report final.txt");
    assert_eq!(parsed.metadata.size, 5);
    assert_eq!(parsed.metadata.modified, Some(1_400_000_000));
}

#[test]
fn an_epoch_style_folder_line_is_a_folder() {
    let parsed = line("drwxr-xr-x  2 1000 1000  4096 1759312496 photos");

    assert_eq!(parsed.name, "photos");
    assert!(parsed.metadata.is_dir());
    assert_eq!(parsed.metadata.modified, Some(1_759_312_496));
}

#[test]
fn a_name_that_looks_like_an_epoch_time_is_still_a_name() {
    let parsed = line("-rw-r--r--  1 1000 1000     5 1400000000 1500000000");

    assert_eq!(parsed.name, "1500000000");
    assert_eq!(parsed.metadata.modified, Some(1_400_000_000));
}

#[test]
fn a_month_name_is_never_mistaken_for_an_epoch_time() {
    let parsed = line("-rw-r--r--  1 1000 1000     5 Mar  1 12:34 a.txt");

    assert_eq!(parsed.name, "a.txt");
    assert!(parsed.metadata.modified.is_some());
}

#[test]
fn an_epoch_of_zero_is_a_listed_file_not_a_dropped_line() {
    let parsed = line("-rw-r--r--  1 1000 1000     5 0 zero.txt");

    assert_eq!(parsed.name, "zero.txt");
    assert_eq!(parsed.metadata.modified, Some(0));
}

#[test]
fn short_epoch_values_are_parsed_at_any_digit_count() {
    let eight = line("-rw-r--r--  1 1000 1000     5 12345678 eight.txt");
    let nine = line("-rw-r--r--  1 1000 1000     5 123456789 nine.txt");

    assert_eq!((eight.name.as_str(), eight.metadata.modified), ("eight.txt", Some(12_345_678)));
    assert_eq!((nine.name.as_str(), nine.metadata.modified), ("nine.txt", Some(123_456_789)));
}

#[test]
fn a_negative_epoch_is_listed_with_time_zero() {
    let parsed = line("-rw-r--r--  1 1000 1000     5 -5 before the epoch.txt");

    assert_eq!(parsed.name, "before the epoch.txt");
    assert_eq!(parsed.metadata.modified, Some(0));
}

#[test]
fn an_epoch_style_device_line_keeps_its_name_and_time() {
    let parsed = line("crw-rw-rw-  1 0 0 1, 3 1400000000 null");

    assert_eq!(parsed.name, "null");
    assert_eq!(parsed.metadata.size, 0);
    assert_eq!(parsed.metadata.modified, Some(1_400_000_000));
}
