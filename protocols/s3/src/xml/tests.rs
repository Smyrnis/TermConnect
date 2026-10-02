use porthmos_vfs::ErrorKind;

use super::*;

const AWS_BUCKETS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner><ID>abc</ID><DisplayName>me</DisplayName></Owner>
  <Buckets>
    <Bucket><Name>photos</Name><CreationDate>2013-05-24T00:00:00.000Z</CreationDate></Bucket>
    <Bucket><Name>backups</Name><CreationDate>2026-09-26T13:51:38.947Z</CreationDate></Bucket>
  </Buckets>
</ListAllMyBucketsResult>"#;

const LISTING: &str = r#"<?xml version="1.0" encoding="UTF-8"?><ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>bkt</Name><Prefix>docs/</Prefix><KeyCount>3</KeyCount><MaxKeys>2</MaxKeys><Delimiter>/</Delimiter><IsTruncated>true</IsTruncated><Contents><Key>docs/</Key><LastModified>2013-05-24T00:00:00.000Z</LastModified><ETag>"d41d8cd98f00b204e9800998ecf8427e"</ETag><Size>0</Size><StorageClass>STANDARD</StorageClass></Contents><Contents><Key>docs/Tom &amp; Jerry.txt</Key><LastModified>2026-09-26T13:51:38.947Z</LastModified><Size>11</Size></Contents><CommonPrefixes><Prefix>docs/sub/</Prefix></CommonPrefixes><NextContinuationToken>token+/=</NextContinuationToken></ListBucketResult>"#;

const MINIO_LISTING: &str = r#"<ListBucketResult><Name>bkt</Name><IsTruncated>false</IsTruncated><Contents><Key>a.txt</Key><Size>5</Size><LastModified>2013-05-24T00:00:00Z</LastModified></Contents></ListBucketResult>"#;

const UPLOADS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><ListMultipartUploadsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Bucket>bkt</Bucket><IsTruncated>true</IsTruncated><NextKeyMarker>big.bin</NextKeyMarker><NextUploadIdMarker>id-2</NextUploadIdMarker><Upload><Key>big.bin</Key><UploadId>id-1</UploadId><Initiated>2013-05-24T00:00:00.000Z</Initiated></Upload><Upload><Key>big.bin</Key><UploadId>id-2</UploadId></Upload><CommonPrefixes><Prefix>sub/</Prefix></CommonPrefixes></ListMultipartUploadsResult>"#;

const PARTS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><ListPartsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Bucket>bkt</Bucket><Key>big.bin</Key><Part><ETag>"0a8d"</ETag><LastModified>2026-09-26T13:51:38.947Z</LastModified><PartNumber>2</PartNumber><Size>10</Size></Part><Part><ETag>"08b4"</ETag><LastModified>2013-05-24T00:00:00.000Z</LastModified><PartNumber>1</PartNumber><Size>5242880</Size></Part><UploadId>id-1</UploadId><IsTruncated>true</IsTruncated><NextPartNumberMarker>2</NextPartNumberMarker></ListPartsResult>"#;

#[test]
fn buckets_are_named_with_creation_times() {
    assert_eq!(
        buckets(AWS_BUCKETS).unwrap(),
        vec![
            Bucket { name: "photos".to_string(), created: Some(1_369_353_600) },
            Bucket { name: "backups".to_string(), created: Some(1_790_430_698) },
        ]
    );
}

#[test]
fn a_listing_has_objects_prefixes_and_a_continuation_token() {
    let listing = listing(LISTING).unwrap();

    assert_eq!(
        listing.objects,
        vec![
            Object { key: "docs/".to_string(), size: 0, modified: Some(1_369_353_600) },
            Object { key: "docs/Tom & Jerry.txt".to_string(), size: 11, modified: Some(1_790_430_698) },
        ]
    );
    assert_eq!(listing.prefixes, vec!["docs/sub/".to_string()]);
    assert_eq!((listing.next.as_deref(), listing.truncated), (Some("token+/="), true));
}

#[test]
fn an_untruncated_listing_without_a_namespace_has_no_token() {
    let listing = listing(MINIO_LISTING).unwrap();

    assert_eq!(listing.objects.len(), 1);
    assert_eq!((listing.next, listing.truncated), (None, false));
}

#[test]
fn unfinished_uploads_are_listed_with_their_markers() {
    let uploads = uploads(UPLOADS).unwrap();

    assert_eq!(
        uploads.uploads,
        vec![
            Upload { key: "big.bin".to_string(), id: "id-1".to_string(), initiated: Some(1_369_353_600) },
            Upload { key: "big.bin".to_string(), id: "id-2".to_string(), initiated: None },
        ]
    );
    assert_eq!(uploads.prefixes, vec!["sub/".to_string()]);
    assert_eq!(uploads.next, Some(("big.bin".to_string(), "id-2".to_string())));
}

#[test]
fn parts_keep_numbers_sizes_etags_and_times() {
    let parts = parts(PARTS).unwrap();

    assert_eq!(
        parts.parts,
        vec![
            Part { number: 2, size: 10, etag: "\"0a8d\"".to_string(), modified: Some(1_790_430_698) },
            Part { number: 1, size: 5_242_880, etag: "\"08b4\"".to_string(), modified: Some(1_369_353_600) },
        ]
    );
    assert_eq!(parts.next, Some(2));
}

#[test]
fn a_new_upload_has_an_id() {
    let xml = r#"<InitiateMultipartUploadResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Bucket>b</Bucket><Key>k</Key><UploadId>abc-123</UploadId></InitiateMultipartUploadResult>"#;

    assert_eq!(upload_id(xml).unwrap(), "abc-123");
    assert_eq!(upload_id("<Other/>").unwrap_err().kind(), ErrorKind::Other);
}

#[test]
fn delete_results_report_the_keys_that_failed() {
    let xml = r#"<DeleteResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Deleted><Key>a</Key></Deleted><Error><Key>b &amp; c</Key><Code>AccessDenied</Code><Message>no</Message></Error></DeleteResult>"#;

    assert_eq!(
        delete_failures(xml).unwrap(),
        vec![DeleteFailure { key: "b & c".to_string(), code: "AccessDenied".to_string() }]
    );
}

#[test]
fn error_bodies_give_code_message_and_region() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?><Error><Code>AuthorizationHeaderMalformed</Code><Message>the region 'us-east-1' is wrong; expecting 'eu-west-1'</Message><Region>eu-west-1</Region><RequestId>1</RequestId></Error>"#;

    assert_eq!(
        error(400, xml),
        S3Error {
            status: 400,
            code: "AuthorizationHeaderMalformed".to_string(),
            message: "the region 'us-east-1' is wrong; expecting 'eu-west-1'".to_string(),
            region: Some("eu-west-1".to_string()),
        }
    );
}

#[test]
fn a_body_that_is_not_an_s3_error_keeps_only_the_status() {
    assert_eq!(error(502, "<html>bad gateway</html>"), S3Error { status: 502, ..S3Error::default() });
    assert_eq!(error(500, ""), S3Error { status: 500, ..S3Error::default() });
}

#[test]
fn a_successful_reply_can_still_carry_an_error() {
    let late = r#"<?xml version="1.0"?><Error><Code>InternalError</Code><Message>try again</Message></Error>"#;
    let done = r#"<CompleteMultipartUploadResult><Key>k</Key></CompleteMultipartUploadResult>"#;

    assert_eq!(completion_error(late).map(|error| error.code), Some("InternalError".to_string()));
    assert_eq!(completion_error(done), None);
}

#[test]
fn request_bodies_are_escaped() {
    assert_eq!(
        complete_body(&[(1, "\"a\"".to_string()), (2, "\"b\"".to_string())]),
        "<CompleteMultipartUpload xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Part><PartNumber>1</PartNumber><ETag>&quot;a&quot;</ETag></Part><Part><PartNumber>2</PartNumber><ETag>&quot;b&quot;</ETag></Part></CompleteMultipartUpload>"
    );
    assert_eq!(
        delete_body(&["a<b>&c".to_string()]),
        "<Delete xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Quiet>true</Quiet><Object><Key>a&lt;b&gt;&amp;c</Key></Object></Delete>"
    );
}

#[test]
fn malformed_xml_is_an_error() {
    for xml in ["not xml <<", "<ListBucketResult><Contents>"] {
        let error = listing(xml).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::Other);
        assert!(error.to_string().starts_with("invalid S3 response"), "{error}");
    }
}

#[test]
fn a_truncated_parts_page_without_a_marker_continues_after_its_highest_part() {
    let xml = r#"<ListPartsResult><Part><PartNumber>1</PartNumber><Size>1</Size></Part><Part><PartNumber>3</PartNumber><Size>1</Size></Part><IsTruncated>true</IsTruncated></ListPartsResult>"#;

    assert_eq!(parts(xml).unwrap().next, Some(3));
}

#[test]
fn listing_cursors_follow_tokens_or_the_last_key() {
    let page = |truncated: bool, token: Option<&str>, keys: &[&str]| Listing {
        objects: keys.iter().map(|key| Object { key: key.to_string(), size: 0, modified: None }).collect(),
        prefixes: Vec::new(),
        next: token.map(str::to_string),
        truncated,
    };

    assert_eq!(page(false, None, &["a"]).cursor(None).unwrap(), None);
    assert_eq!(
        page(true, Some("t2"), &["a"]).cursor(Some(&Cursor::Token("t1".to_string()))).unwrap(),
        Some(Cursor::Token("t2".to_string()))
    );
    assert_eq!(page(true, None, &["a", "b"]).cursor(None).unwrap(), Some(Cursor::StartAfter("b".to_string())));
    assert!(page(true, Some("t1"), &["a"]).cursor(Some(&Cursor::Token("t1".to_string()))).is_err());
    assert!(page(true, None, &[]).cursor(None).is_err());
    assert!(page(true, None, &["b"]).cursor(Some(&Cursor::StartAfter("b".to_string()))).is_err());
    let with_prefix = Listing { prefixes: vec!["c/".to_string()], ..page(true, None, &["a"]) };
    assert_eq!(with_prefix.cursor(None).unwrap(), Some(Cursor::StartAfter("c0".to_string())));
}

#[test]
fn a_copied_part_reports_its_etag() {
    let xml =
        r#"<CopyPartResult><LastModified>2013-05-24T00:00:00.000Z</LastModified><ETag>"abc"</ETag></CopyPartResult>"#;

    assert_eq!(copy_part_etag(xml), Some("\"abc\"".to_string()));
    assert_eq!(copy_part_etag("<Error><Code>InternalError</Code></Error>"), None);
}
