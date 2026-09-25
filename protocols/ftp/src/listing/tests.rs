use porthmos_vfs::FileKind;

use super::*;

const NOW: u64 = 1_788_000_000;

#[test]
fn mlsd_reads_type_size_and_modify_time() {
    let item = parse_mlsd_line("type=file;size=1024;modify=20240301123456; report 2024.pdf").unwrap();

    assert_eq!(item.name, "report 2024.pdf");
    assert_eq!((item.metadata.kind, item.metadata.size), (FileKind::File, 1024));
    assert_eq!(item.metadata.modified, Some(1_709_296_496));
}

#[test]
fn mlsd_fractional_seconds_are_ignored() {
    let item = parse_mlsd_line("type=file;size=1;modify=20240301123456.789; a").unwrap();

    assert_eq!(item.metadata.modified, Some(1_709_296_496));
}

#[test]
fn mlsd_directories_and_links_are_typed() {
    assert_eq!(parse_mlsd_line("type=dir;modify=20240301000000; photos").unwrap().metadata.kind, FileKind::Dir);
    assert_eq!(parse_mlsd_line("type=OS.unix=symlink; latest").unwrap().metadata.kind, FileKind::Symlink);
}

#[test]
fn mlsd_current_and_parent_entries_are_dropped() {
    assert!(parse_mlsd_line("type=cdir; .").is_none());
    assert!(parse_mlsd_line("type=pdir; ..").is_none());
}

#[test]
fn unix_list_lines_with_time_or_year_are_parsed() {
    let recent = parse_list_line("-rw-r--r--    1 1000     1000         4096 Mar  1 12:34 notes 2.txt", NOW).unwrap();
    assert_eq!(
        (recent.name.as_str(), recent.metadata.size, recent.metadata.kind),
        ("notes 2.txt", 4096, FileKind::File)
    );
    assert_eq!(recent.metadata.modified, Some(1_772_368_440));
    assert_eq!(recent.metadata.permissions, Some(0o644));

    let old = parse_list_line("drwxr-xr-x    2 ftp      ftp             0 Jan 15  2019 2019", NOW).unwrap();
    assert_eq!((old.name.as_str(), old.metadata.kind), ("2019", FileKind::Dir));
    assert_eq!(old.metadata.modified, Some(1_547_510_400));
}

#[test]
fn unix_list_symlinks_keep_their_target() {
    let link = parse_list_line("lrwxrwxrwx 1 root root 7 Mar  1 12:34 latest -> v1.2.3", NOW).unwrap();

    assert_eq!((link.name.as_str(), link.metadata.kind), ("latest", FileKind::Symlink));
    assert_eq!(link.link_target.as_deref(), Some("v1.2.3"));
}

#[test]
fn windows_list_lines_are_parsed() {
    let dir = parse_list_line("03-01-24  12:34PM       <DIR>          Backups", NOW).unwrap();
    assert_eq!((dir.name.as_str(), dir.metadata.kind), ("Backups", FileKind::Dir));
    assert_eq!(dir.metadata.modified, Some(1_709_296_440));

    let file = parse_list_line("03-01-24  09:05AM                 2048 ñandú fish.bin", NOW).unwrap();
    assert_eq!((file.name.as_str(), file.metadata.size, file.metadata.kind), ("ñandú fish.bin", 2048, FileKind::File));
}

#[test]
fn unix_list_dot_entries_and_total_lines_are_dropped() {
    assert!(parse_list_line("total 12", NOW).is_none());
    assert!(parse_list_line("drwxr-xr-x 2 u g 0 Mar  1 12:34 .", NOW).is_none());
    assert!(parse_list_line("drwxr-xr-x 2 u g 0 Mar  1 12:34 ..", NOW).is_none());
}

#[test]
fn a_name_that_starts_with_digits_or_spaces_is_kept_exactly() {
    let item = parse_list_line("-rw-r--r-- 1 u g 10 Mar  1 12:34  2 leading.txt", NOW).unwrap();

    assert_eq!(item.name, " 2 leading.txt");
}

#[test]
fn mlst_facts_give_metadata() {
    let metadata = parse_mlst_facts(" type=file;size=5;modify=19700101000010; /a.txt").unwrap();

    assert_eq!((metadata.size, metadata.modified, metadata.kind), (5, Some(10), FileKind::File));
}

#[test]
fn the_year_of_a_day_count_follows_the_calendar() {
    assert_eq!(civil_year_from_days(0), 1970);
    assert_eq!(civil_year_from_days(days_from_civil(2024, 12, 31)), 2024);
    assert_eq!(civil_year_from_days(days_from_civil(2025, 1, 1)), 2025);
    assert_eq!(civil_year_from_days(days_from_civil(2000, 2, 29)), 2000);
}

#[test]
fn a_date_without_a_year_that_would_be_in_the_future_belongs_to_last_year() {
    let january_second_2026 = 1_767_312_000;

    let item = parse_list_line("-rw-r--r-- 1 u g 1 Dec 31 23:00 late.txt", january_second_2026).unwrap();

    assert_eq!(item.metadata.modified, Some(1_767_222_000));
}

#[test]
fn windows_lines_with_four_digit_years_and_24_hour_times_are_parsed() {
    let file = parse_list_line("03-01-2024  14:34                 2048 report.pdf", NOW).unwrap();

    assert_eq!((file.name.as_str(), file.metadata.size), ("report.pdf", 2048));
    assert_eq!(file.metadata.modified, Some(1_709_303_640));
}

#[test]
fn unix_lines_without_a_group_column_are_parsed() {
    let item = parse_list_line("-rw-r--r--   1 owner     4096 Mar  1 12:34 no group.txt", NOW).unwrap();

    assert_eq!((item.name.as_str(), item.metadata.size), ("no group.txt", 4096));
}

#[test]
fn mlst_of_the_current_or_parent_directory_is_a_directory() {
    assert_eq!(parse_mlst_facts(" type=cdir;modify=20240301000000; /home/u").unwrap().kind, FileKind::Dir);
    assert_eq!(parse_mlst_facts(" type=pdir; /home").unwrap().kind, FileKind::Dir);
}
