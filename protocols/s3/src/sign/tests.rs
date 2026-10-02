use super::*;

const EMPTY_HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
}

fn signature(header: &str) -> &str {
    header.rsplit("Signature=").next().unwrap()
}

#[test]
fn the_empty_payload_hash_is_the_sha256_of_nothing() {
    assert_eq!(payload_hash(b""), EMPTY_HASH);
}

#[test]
fn aws_test_suite_get_vanilla() {
    let headers = headers(&[("host", "example.amazonaws.com"), ("x-amz-date", "20150830T123600Z")]);
    let request = Signable { method: "GET", path: "/", query: &[], headers: &headers, payload_hash: EMPTY_HASH };
    let credentials = Credentials {
        access_key: "AKIDEXAMPLE".to_string(),
        secret: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
    };

    let header = authorization(&request, &credentials, "us-east-1", "service", "20150830T123600Z");

    assert!(header.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, "), "{header}");
    assert_eq!(signature(&header), "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31");
}

#[test]
fn s3_documentation_get_object_example() {
    let headers = headers(&[
        ("host", "examplebucket.s3.amazonaws.com"),
        ("range", "bytes=0-9"),
        ("x-amz-content-sha256", EMPTY_HASH),
        ("x-amz-date", "20130524T000000Z"),
    ]);
    let request =
        Signable { method: "GET", path: "/test.txt", query: &[], headers: &headers, payload_hash: EMPTY_HASH };
    let credentials = Credentials {
        access_key: "AKIAIOSFODNN7EXAMPLE".to_string(),
        secret: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
    };

    let header = authorization(&request, &credentials, "us-east-1", "s3", "20130524T000000Z");

    assert!(header.contains("SignedHeaders=host;range;x-amz-content-sha256;x-amz-date,"), "{header}");
    assert_eq!(signature(&header), "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41");
}

#[test]
fn query_parameters_are_encoded_and_sorted() {
    let query = vec![
        ("prefix".to_string(), "a b".to_string()),
        ("list-type".to_string(), "2".to_string()),
        ("uploads".to_string(), String::new()),
    ];

    assert_eq!(canonical_query(&query), "list-type=2&prefix=a%20b&uploads=");
}

#[test]
fn header_names_are_sorted_and_values_trimmed() {
    let headers = headers(&[("x-amz-date", " 20150830T123600Z "), ("host", "h")]);
    let request = Signable { method: "PUT", path: "/a%20b", query: &[], headers: &headers, payload_hash: EMPTY_HASH };

    let (canonical, signed) = canonical_request(&request);

    assert_eq!(signed, "host;x-amz-date");
    assert_eq!(
        canonical,
        format!("PUT\n/a%20b\n\nhost:h\nx-amz-date:20150830T123600Z\n\nhost;x-amz-date\n{EMPTY_HASH}")
    );
}
