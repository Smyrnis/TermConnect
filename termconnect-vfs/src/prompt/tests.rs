use super::*;

#[test]
fn answer_debug_redacts_the_password() {
    assert!(!format!("{:?}", Answer::Password("hunter2".into())).contains("hunter2"));
}
