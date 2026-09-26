use std::path::PathBuf;

use porthmos_vfs::{Choice, OptionKind, Protocol};

use super::*;

fn webdav() -> WebDav {
    WebDav::new(PathBuf::from("/tmp/known_certificates.toml"))
}

#[test]
fn describes_itself_as_webdav_on_port_443() {
    let protocol = webdav();

    assert_eq!((protocol.id(), protocol.display_name(), protocol.default_port()), ("webdav", "WebDAV", 443));
}

#[test]
fn the_form_offers_security_and_root_with_an_optional_username() {
    let form = webdav().connection_form();

    assert_eq!(form.port.default, 443);
    assert!(!form.username.required);
    assert!(!form.password.required);
    assert_eq!(
        form.option("security").map(|field| field.kind.clone()),
        Some(OptionKind::Choice {
            choices: &[
                Choice { value: "https", label: "HTTPS" },
                Choice { value: "http", label: "HTTP (unencrypted)" },
            ],
            default: "https",
        })
    );
    assert_eq!(
        form.option("root").map(|field| (field.label, field.kind.clone())),
        Some(("Root path", OptionKind::Text { default: "/" }))
    );
    assert!(form.reserved_key_collisions().is_empty());
}
