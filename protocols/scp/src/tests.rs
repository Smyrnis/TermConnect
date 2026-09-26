use porthmos_vfs::{Environment, Protocol};

use super::*;

#[test]
fn describes_itself_as_scp_on_port_22() {
    let protocol = Scp::default();

    assert_eq!((protocol.id(), protocol.display_name(), protocol.default_port()), ("scp", "SCP", 22));
}

#[test]
fn the_form_matches_sftp_with_an_identity_file() {
    let form = Scp::default().connection_form();

    assert_eq!(form.option("identity_file").map(|field| field.label), Some("Identity file"));
    assert!(form.reserved_key_collisions().is_empty());
}

#[test]
fn scp_does_not_discover_hosts() {
    assert!(Scp::default().discover(&Environment::default()).unwrap().is_empty());
}
