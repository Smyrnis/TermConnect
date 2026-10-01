use std::path::PathBuf;

use porthmos_vfs::{Environment, OptionKind, Protocol, Target};

use super::*;

fn ftp() -> Ftp {
    Ftp::new(PathBuf::from("/k.toml"))
}

#[test]
fn the_ftp_form_has_security_and_passive_options_and_an_optional_username() {
    let form = ftp().connection_form();

    assert_eq!(form.port.default, 21);
    assert!(!form.username.required);
    assert!(matches!(
        form.option("security").map(|field| &field.kind),
        Some(OptionKind::Choice { default: "explicit", .. })
    ));
    assert!(matches!(form.option("passive").map(|field| &field.kind), Some(OptionKind::Toggle { default: true })));
    assert!(form.reserved_key_collisions().is_empty());
}

#[test]
fn ftp_identifies_itself_and_offers_no_shell() {
    let ftp = ftp();
    let target = Target {
        name: "n".into(),
        host: "h".into(),
        port: 21,
        username: "u".into(),
        password: None,
        options: Default::default(),
    };

    assert_eq!((ftp.id(), ftp.display_name(), ftp.default_port()), ("ftp", "FTP", 21));
    assert!(ftp.shell_command(&target, &Environment::default()).is_none());
}

#[test]
fn mfmt_stamps_are_utc_without_separators() {
    use crate::fs::mfmt_stamp;

    assert_eq!(mfmt_stamp(0).unwrap(), "19700101000000");
    assert_eq!(mfmt_stamp(1_700_000_000).unwrap(), "20231114221320");
    assert_eq!(mfmt_stamp(951_782_400).unwrap(), "20000229000000");
}

#[test]
fn mfmt_stamps_stop_at_the_last_second_of_year_9999() {
    use crate::fs::mfmt_stamp;

    assert_eq!(mfmt_stamp(253_402_300_799).unwrap(), "99991231235959");
    for beyond in [253_402_300_800, u64::from(u32::MAX) * 100_000, u64::MAX] {
        let error = mfmt_stamp(beyond).unwrap_err();
        assert_eq!(error.kind(), porthmos_vfs::ErrorKind::Other, "{beyond}");
        assert!(error.to_string().contains("year 9999"), "{error}");
    }
}
