use super::*;

#[test]
fn the_standard_form_has_the_classic_fields_and_no_options() {
    let form = ConnectionForm::standard(22);

    assert_eq!(form.host, CommonField { label: "Host", required: true });
    assert_eq!(form.port, PortField { label: "Port", default: 22 });
    assert_eq!(form.username, CommonField { label: "Username", required: true });
    assert_eq!(form.password, CommonField { label: "Password", required: false });
    assert!(form.options.is_empty());
}

#[test]
fn reserved_key_collisions_lists_options_that_reuse_a_form_key() {
    let mut form = ConnectionForm::standard(21);
    form.options = vec![
        OptionField { key: "host", label: "Host again", required: false, kind: OptionKind::Secret },
        OptionField { key: "bucket", label: "Bucket", required: true, kind: OptionKind::Text { default: "" } },
    ];

    assert_eq!(form.reserved_key_collisions(), ["host"]);
}

#[test]
fn option_finds_a_described_field_by_key() {
    let mut form = ConnectionForm::standard(21);
    form.options =
        vec![OptionField { key: "tls", label: "TLS", required: false, kind: OptionKind::Toggle { default: true } }];

    assert_eq!(form.option("tls").map(|field| field.label), Some("TLS"));
    assert!(form.option("missing").is_none());
}
