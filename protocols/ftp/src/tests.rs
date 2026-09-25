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
