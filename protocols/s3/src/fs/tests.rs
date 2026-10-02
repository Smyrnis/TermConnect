use super::*;

#[test]
fn large_copies_are_split_into_consecutive_ranges() {
    assert_eq!(copy_ranges(10, 4), vec![(0, 3), (4, 7), (8, 9)]);
    assert_eq!(copy_ranges(8, 4), vec![(0, 3), (4, 7)]);
    assert_eq!(copy_ranges(0, 4), Vec::<(u64, u64)>::new());
}
