use std::collections::BTreeMap;

use porthmos_vfs::Target;

use super::*;

fn target(username: &str, password: Option<&str>, options: &[(&str, &str)]) -> Target {
    Target {
        name: "nas".into(),
        host: "nas.local".into(),
        port: 21,
        username: username.into(),
        password: password.map(str::to_string),
        options: options.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn an_empty_username_logs_in_anonymously() {
    let settings = FtpSettings::from_target(&target("", None, &[]));

    assert_eq!((settings.username.as_str(), settings.password.as_deref()), ("anonymous", Some("anonymous@")));
    assert!(settings.is_anonymous());
}

#[test]
fn security_and_passive_default_to_explicit_tls_and_passive_mode() {
    let settings = FtpSettings::from_target(&target("u", Some("p"), &[]));

    assert_eq!(settings.security, Security::Explicit);
    assert!(settings.passive);
    assert!(!settings.is_anonymous());
}

#[test]
fn saved_security_and_passive_choices_are_read() {
    let plain = FtpSettings::from_target(&target("u", None, &[("security", "plain"), ("passive", "false")]));
    assert_eq!((plain.security, plain.passive), (Security::Plain, false));

    let implicit = FtpSettings::from_target(&target("u", None, &[("security", "implicit")]));
    assert_eq!(implicit.security, Security::Implicit);
}
