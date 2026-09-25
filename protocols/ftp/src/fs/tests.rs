use super::*;

#[test]
fn a_replayed_mkdir_or_removal_that_already_happened_counts_as_done() {
    assert!(already_done(SimpleCommand::MakeDir, true));
    assert!(!already_done(SimpleCommand::MakeDir, false));
    assert!(already_done(SimpleCommand::RemoveFile, false));
    assert!(already_done(SimpleCommand::RemoveDir, false));
    assert!(!already_done(SimpleCommand::RemoveFile, true));
}
