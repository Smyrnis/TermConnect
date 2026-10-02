use porthmos_vfs::testing::{FakeFs, FakeProtocol};

use super::*;

#[test]
fn protocol_info_copies_the_protocols_id_name_and_form() {
    let mut form = ConnectionForm::standard(21);
    form.host.label = "Server";
    let protocol = FakeProtocol::new(FakeFs::new()).with_id("ftp").with_form(form.clone());

    let info = ProtocolInfo::from_protocol(&protocol);

    assert_eq!((info.id, info.form), ("ftp", form));
    assert_eq!(info.display_name, protocol.display_name());
}
