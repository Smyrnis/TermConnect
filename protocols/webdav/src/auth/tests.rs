use super::*;

const RFC_CHALLENGE: &str = r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=ALGO, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#;
const RFC_CNONCE: &str = "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ";

fn rfc_authenticator(algorithm: &str) -> Authenticator {
    let scheme = strongest(challenges([RFC_CHALLENGE.replace("ALGO", algorithm).as_str()])).unwrap();
    let mut authenticator = Authenticator::default();
    authenticator.use_scheme(scheme);
    authenticator.use_credentials("Mufasa", "Circle of Life");
    authenticator
}

fn digest(realm: &str, algorithm: &str, stale: bool) -> String {
    format!(r#"Digest realm="{realm}", nonce="n-{realm}", qop="auth", algorithm={algorithm}, stale={stale}"#)
}

#[test]
fn a_basic_challenge_is_recognised() {
    assert_eq!(challenges([r#"Basic realm="nas""#]), vec![Challenge::Basic]);
}

#[test]
fn several_challenges_in_one_header_keep_quoted_commas() {
    let found = challenges([r#"Basic realm="a, b", Digest realm="r", nonce="n", qop="auth, auth-int""#]);

    assert_eq!(found.len(), 2);
    assert_eq!(found[0], Challenge::Basic);
    assert!(matches!(&found[1], Challenge::Digest(header) if header.realm == "r" && header.nonce == "n"));
}

#[test]
fn unknown_schemes_are_named() {
    assert_eq!(
        challenges(["Negotiate", r#"Bearer realm="x""#]),
        vec![Challenge::Unsupported("Negotiate".to_string()), Challenge::Unsupported("Bearer".to_string())]
    );
}

#[test]
fn the_strongest_supported_scheme_wins() {
    let offered = challenges([
        r#"Basic realm="x""#,
        digest("md5", "MD5", false).as_str(),
        digest("sha", "SHA-256", false).as_str(),
    ]);

    assert!(matches!(strongest(offered), Ok(Scheme::Digest(header)) if header.realm == "sha"));
    assert!(matches!(
        strongest(challenges([r#"Basic realm="x""#, digest("md5", "MD5", false).as_str()])),
        Ok(Scheme::Digest(header)) if header.realm == "md5"
    ));
    assert_eq!(strongest(challenges([r#"Basic realm="x""#])), Ok(Scheme::Basic));
}

#[test]
fn only_unsupported_schemes_is_an_error_listing_them() {
    assert_eq!(
        strongest(challenges(["Negotiate", r#"Bearer realm="x""#])),
        Err(vec!["Negotiate".to_string(), "Bearer".to_string()])
    );
}

#[test]
fn basic_credentials_are_base64_encoded() {
    let mut authenticator = Authenticator::default();
    authenticator.use_scheme(Scheme::Basic);
    authenticator.use_credentials("u", "p");

    assert_eq!(authenticator.header("GET", "/"), Some("Basic dTpw".to_string()));
}

#[test]
fn no_header_without_credentials_or_scheme() {
    let mut authenticator = Authenticator::default();
    assert_eq!(authenticator.header("GET", "/"), None);

    authenticator.use_credentials("u", "p");
    assert_eq!(authenticator.header("GET", "/"), None);
}

#[test]
fn digest_sha256_matches_rfc_7616() {
    let header = rfc_authenticator("SHA-256").header_with_cnonce("GET", "/dir/index.html", Some(RFC_CNONCE)).unwrap();

    assert!(header.contains("753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1"), "{header}");
    assert!(header.contains("nc=00000001"), "{header}");
}

#[test]
fn digest_md5_matches_rfc_7616() {
    let header = rfc_authenticator("MD5").header_with_cnonce("GET", "/dir/index.html", Some(RFC_CNONCE)).unwrap();

    assert!(header.contains("8ca523f5e9506fed4657c9700eebdbec"), "{header}");
}

#[test]
fn every_digest_header_counts_up() {
    let mut authenticator = rfc_authenticator("MD5");
    authenticator.header("PROPFIND", "/a/");

    assert!(authenticator.header("PROPFIND", "/a/").unwrap().contains("nc=00000002"));
}

#[test]
fn a_stale_digest_challenge_replaces_the_nonce() {
    let mut authenticator = rfc_authenticator("MD5");

    assert!(authenticator.refresh(challenges([digest("fresh", "MD5", true).as_str()])));
    let header = authenticator.header("GET", "/").unwrap();
    assert!(header.contains(r#"nonce="n-fresh""#) && header.contains("nc=00000001"), "{header}");
}

#[test]
fn a_non_stale_challenge_or_basic_scheme_is_not_refreshed() {
    let mut digest_user = rfc_authenticator("MD5");
    assert!(!digest_user.refresh(challenges([digest("other", "MD5", false).as_str()])));

    let mut basic_user = Authenticator::default();
    basic_user.use_scheme(Scheme::Basic);
    assert!(!basic_user.refresh(challenges([digest("other", "MD5", true).as_str()])));
}

#[test]
fn offered_auth_and_auth_int_are_answered_with_plain_auth() {
    let header = rfc_authenticator("MD5").header_with_cnonce("GET", "/dir/index.html", Some(RFC_CNONCE)).unwrap();

    assert!(header.contains("qop=auth,"), "{header}");
    assert!(!header.contains("auth-int"), "{header}");
}

#[test]
fn sha256_digest_beats_md5_sess() {
    let offered = challenges([digest("sha", "SHA-256", false).as_str(), digest("sess", "MD5-sess", false).as_str()]);

    assert!(matches!(strongest(offered), Ok(Scheme::Digest(header)) if header.realm == "sha"));
}

#[test]
fn a_first_challenge_is_adopted_when_credentials_are_known() {
    let mut authenticator = Authenticator::default();
    authenticator.use_credentials("u", "p");

    assert!(authenticator.refresh(challenges([r#"Basic realm="x""#])));
    assert_eq!(authenticator.header("PUT", "/f"), Some("Basic dTpw".to_string()));
}

#[test]
fn a_first_challenge_is_not_adopted_without_credentials() {
    let mut authenticator = Authenticator::default();

    assert!(!authenticator.refresh(challenges([r#"Basic realm="x""#])));
    assert_eq!(authenticator.header("PUT", "/f"), None);
}

#[test]
fn a_second_stale_challenge_for_the_current_nonce_keeps_counting() {
    let mut authenticator = rfc_authenticator("MD5");
    let stale = digest("fresh", "MD5", true);
    assert!(authenticator.refresh(challenges([stale.as_str()])));
    authenticator.header("GET", "/");

    assert!(authenticator.refresh(challenges([stale.as_str()])));

    assert!(authenticator.header("GET", "/").unwrap().contains("nc=00000002"));
}
