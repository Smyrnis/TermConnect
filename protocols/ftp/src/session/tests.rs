use porthmos_vfs::ErrorKind;

use super::*;

fn error() -> ProtocolError {
    ProtocolError::new(ErrorKind::Connect, anyhow!("tls handshake eof"))
}

#[test]
fn implicit_tls_on_port_21_hints_at_port_990() {
    let hinted = with_port_hint(error(), Security::Implicit, 21);

    assert_eq!(hinted.to_string(), "tls handshake eof — implicit TLS usually uses port 990");
    assert_eq!(hinted.kind(), ErrorKind::Connect);
}

#[test]
fn other_security_modes_or_ports_get_no_hint() {
    assert_eq!(with_port_hint(error(), Security::Implicit, 990).to_string(), "tls handshake eof");
    assert_eq!(with_port_hint(error(), Security::Explicit, 21).to_string(), "tls handshake eof");
}

#[test]
fn the_passive_address_is_replaced_only_for_servers_outside_the_local_network() {
    assert!(uses_nat_workaround("203.0.113.7".parse().unwrap()));
    assert!(!uses_nat_workaround("192.168.1.10".parse().unwrap()));
    assert!(!uses_nat_workaround("10.0.0.2".parse().unwrap()));
    assert!(!uses_nat_workaround("127.0.0.1".parse().unwrap()));
    assert!(!uses_nat_workaround("::1".parse().unwrap()));
}

#[tokio::test]
async fn a_trust_problem_from_an_earlier_handshake_does_not_label_a_later_network_error() {
    let dir = tempfile::tempdir().unwrap();
    let store = porthmos_tls::KnownCertificates::new(dir.path().join("k.toml"));
    let (tls, problem) = porthmos_tls::client_config("127.0.0.1", 1, store);
    *problem.lock().unwrap() = Some(TrustProblem::Changed);
    let settings = crate::settings::FtpSettings::from_target(&porthmos_vfs::Target {
        name: "n".into(),
        host: "127.0.0.1".into(),
        port: 1,
        username: "u".into(),
        password: None,
        options: [("security".to_string(), "plain".to_string())].into_iter().collect(),
    });

    let result = open(&SessionContext { settings, tls, problem, timeout: DEFAULT_TIMEOUT }).await;

    assert!(matches!(result, Err(OpenError::Other(_))));
}
