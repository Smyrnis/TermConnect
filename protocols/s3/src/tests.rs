use std::path::PathBuf;

use porthmos_vfs::{Choice, OptionKind, Protocol};

use super::*;

fn s3() -> S3 {
    S3::new(PathBuf::from("/tmp/known_certificates.toml"))
}

#[test]
fn describes_itself_as_s3_on_port_443() {
    let protocol = s3();

    assert_eq!((protocol.id(), protocol.display_name(), protocol.default_port()), ("s3", "S3", 443));
}

#[test]
fn the_form_names_the_s3_fields() {
    let form = s3().connection_form();

    assert_eq!((form.host.label, form.host.required), ("Endpoint", true));
    assert_eq!((form.username.label, form.username.required), ("Access key ID", true));
    assert_eq!((form.password.label, form.password.required), ("Secret access key", false));
    assert_eq!(form.port.default, 443);
    assert_eq!(
        form.option("security").map(|field| field.kind.clone()),
        Some(OptionKind::Choice {
            choices: &[
                Choice { value: "https", label: "HTTPS" },
                Choice { value: "http", label: "HTTP (unencrypted)" }
            ],
            default: "https",
        })
    );
    assert_eq!(
        form.option("region").map(|field| (field.label, field.kind.clone())),
        Some(("Region", OptionKind::Text { default: "us-east-1" }))
    );
    assert_eq!(
        form.option("bucket").map(|field| (field.label, field.kind.clone())),
        Some(("Bucket", OptionKind::Text { default: "" }))
    );
    assert_eq!(
        form.option("addressing").map(|field| (field.label, field.kind.clone())),
        Some((
            "Addressing",
            OptionKind::Choice {
                choices: &[
                    Choice { value: "auto", label: "Automatic" },
                    Choice { value: "path", label: "Path-style (host/bucket)" },
                    Choice { value: "virtual", label: "Virtual-hosted (bucket.host)" },
                ],
                default: "auto",
            }
        ))
    );
    assert!(form.reserved_key_collisions().is_empty());
}
