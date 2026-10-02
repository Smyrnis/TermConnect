use super::*;

fn part(number: u32, size: u64, modified: u64) -> Part {
    Part { number, size, etag: format!("\"e{number}\""), modified: Some(modified) }
}

fn upload(id: &str, key: &str, initiated: Option<u64>) -> Upload {
    Upload { key: key.to_string(), id: id.to_string(), initiated }
}

#[test]
fn staged_keys_drop_the_part_suffix() {
    assert_eq!(staged_key("dir/big.bin.part"), Some("dir/big.bin"));
    assert_eq!(staged_key("big.bin"), None);
    assert_eq!(staged_key(".part"), None);
    assert_eq!(staged_key("dir/.part"), None);
}

const MIB: u64 = 1024 * 1024;
const FULL: u64 = 16 * MIB;

#[test]
fn progress_counts_only_full_parts_contiguous_from_one() {
    let parts = [part(2, FULL, 20), part(1, FULL, 30), part(4, FULL, 40)];

    assert_eq!(contiguous(&parts), Progress { last: 2, size: 2 * FULL, modified: Some(30) });
}

#[test]
fn a_part_of_any_other_size_ends_the_resumable_progress() {
    let parts = [part(1, FULL, 10), part(2, 6 * MIB, 20), part(3, FULL, 30)];

    assert_eq!(contiguous(&parts), Progress { last: 1, size: FULL, modified: Some(10) });
}

#[test]
fn a_short_part_ends_the_resumable_progress_but_not_the_completion() {
    let parts = [part(1, FULL, 10), part(2, MIB, 20)];

    assert_eq!(contiguous(&parts), Progress { last: 1, size: FULL, modified: Some(10) });
    assert_eq!(completed(&parts), vec![(1, "\"e1\"".to_string()), (2, "\"e2\"".to_string())]);
}

#[test]
fn completion_uses_the_contiguous_parts() {
    let parts = [part(2, 10, 20), part(1, 8, 30), part(4, 5, 40)];

    assert_eq!(completed(&parts), vec![(1, "\"e1\"".to_string()), (2, "\"e2\"".to_string())]);
}

#[test]
fn progress_of_no_parts_is_empty() {
    assert_eq!(contiguous(&[]), Progress { last: 0, size: 0, modified: None });
    assert_eq!(contiguous(&[part(2, FULL, 20)]), Progress { last: 0, size: 0, modified: None });
    assert_eq!(contiguous(&[part(1, MIB, 20)]), Progress { last: 0, size: 0, modified: None });
}

#[test]
fn the_newest_upload_of_the_key_is_chosen() {
    let uploads = [
        upload("old", "a", Some(1)),
        upload("new", "a", Some(5)),
        upload("other", "b", Some(9)),
        upload("undated", "a", None),
    ];

    assert_eq!(newest(&uploads, "a").map(|found| found.id.as_str()), Some("new"));
    assert_eq!(newest(&uploads, "c"), None);
}

#[test]
fn an_upload_continues_only_from_its_exact_size() {
    let progress = Progress { last: 2, size: 18, modified: None };

    assert_eq!(
        decide(18, Some(("id".to_string(), progress.clone()))),
        Start::Continue { id: "id".to_string(), next_part: 3 }
    );
    assert_eq!(decide(17, Some(("id".to_string(), progress.clone()))), Start::Restart);
    assert_eq!(decide(0, Some(("id".to_string(), progress))), Start::Restart);
    assert_eq!(decide(18, None), Start::Restart);
}
