use porthmos_vfs::{ErrorKind, FileKind};

use super::*;

const SEPT_26_12_10_02: u64 = 1_790_424_602;
const SEPT_01_10_00_00: u64 = 1_788_256_800;

const NEXTCLOUD: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns">
 <d:response>
  <d:href>/remote.php/dav/files/alice/</d:href>
  <d:propstat>
   <d:prop>
    <d:resourcetype><d:collection/></d:resourcetype>
    <d:getlastmodified>Sat, 26 Sep 2026 12:10:02 GMT</d:getlastmodified>
   </d:prop>
   <d:status>HTTP/1.1 200 OK</d:status>
  </d:propstat>
  <d:propstat>
   <d:prop><d:getcontentlength/></d:prop>
   <d:status>HTTP/1.1 404 Not Found</d:status>
  </d:propstat>
 </d:response>
 <d:response>
  <d:href>/remote.php/dav/files/alice/Tom%20&amp;%20Jerry.txt</d:href>
  <d:propstat>
   <d:prop>
    <d:resourcetype/>
    <d:getcontentlength>11</d:getcontentlength>
    <oc:size>99</oc:size>
   </d:prop>
   <d:status>HTTP/1.1 200 OK</d:status>
  </d:propstat>
 </d:response>
</d:multistatus>"#;

const APACHE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:ns0="DAV:">
<D:response xmlns:lp1="DAV:" xmlns:lp2="http://apache.org/dav/props/">
<D:href>/dav/docs/</D:href>
<D:propstat>
<D:prop>
<lp1:resourcetype><D:collection/></lp1:resourcetype>
<lp1:getlastmodified>Tue, 01 Sep 2026 10:00:00 GMT</lp1:getlastmodified>
</D:prop>
<D:status>HTTP/1.1 200 OK</D:status>
</D:propstat>
</D:response>
<D:response xmlns:lp1="DAV:" xmlns:lp2="http://apache.org/dav/props/">
<D:href>/dav/docs/report%202026.pdf</D:href>
<D:propstat>
<D:prop>
<lp1:resourcetype/>
<lp1:getcontentlength>2048</lp1:getcontentlength>
<lp1:getlastmodified>Tue, 01 Sep 2026 10:00:00 GMT</lp1:getlastmodified>
</D:prop>
<D:status>HTTP/1.1 200 OK</D:status>
</D:propstat>
</D:response>
</D:multistatus>"#;

const DEFAULT_NAMESPACE: &str = r#"<multistatus xmlns="DAV:"><response><href>https://nas.local:5006/home/a%C3%BC.txt</href><propstat><prop><resourcetype></resourcetype><getcontentlength>5</getcontentlength><getlastmodified>not a date</getlastmodified></prop><status>HTTP/1.1 200 OK</status></propstat></response></multistatus>"#;

#[test]
fn nextcloud_listing_marks_collections_and_ignores_missing_props() {
    let resources = parse_multistatus(NEXTCLOUD).unwrap();

    assert_eq!(
        resources,
        vec![
            Resource {
                href: "/remote.php/dav/files/alice/".to_string(),
                status: None,
                collection: true,
                size: None,
                modified: Some(SEPT_26_12_10_02),
            },
            Resource {
                href: "/remote.php/dav/files/alice/Tom%20&%20Jerry.txt".to_string(),
                status: None,
                collection: false,
                size: Some(11),
                modified: None,
            },
        ]
    );
}

#[test]
fn apache_live_property_prefixes_are_resolved_as_dav() {
    let resources = parse_multistatus(APACHE).unwrap();

    assert_eq!(resources.len(), 2);
    assert!(resources[0].collection);
    assert_eq!(resources[1].href, "/dav/docs/report%202026.pdf");
    assert_eq!((resources[1].size, resources[1].modified), (Some(2048), Some(SEPT_01_10_00_00)));
}

#[test]
fn a_default_namespace_and_absolute_hrefs_are_understood() {
    let resources = parse_multistatus(DEFAULT_NAMESPACE).unwrap();

    assert_eq!(resources[0].href, "https://nas.local:5006/home/a%C3%BC.txt");
    assert_eq!((resources[0].size, resources[0].modified, resources[0].collection), (Some(5), None, false));
}

#[test]
fn a_response_level_status_is_kept() {
    let resources = parse_multistatus(
        r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/a/locked.txt</d:href><d:status>HTTP/1.1 423 Locked</d:status></d:response></d:multistatus>"#,
    )
    .unwrap();

    assert_eq!(resources[0].status, Some(423));
}

#[test]
fn metadata_reports_kind_size_and_time_without_permissions() {
    let file = Resource { size: Some(7), modified: Some(9), ..Resource::default() };
    let dir = Resource { collection: true, ..Resource::default() };

    assert_eq!(
        (file.metadata().kind, file.metadata().size, file.metadata().modified, file.metadata().permissions),
        (FileKind::File, 7, Some(9), None)
    );
    assert_eq!((dir.metadata().kind, dir.metadata().size), (FileKind::Dir, 0));
}

#[test]
fn malformed_and_truncated_xml_are_errors() {
    for xml in ["not xml <<", r#"<d:multistatus xmlns:d="DAV:"><d:response>"#] {
        let error = parse_multistatus(xml).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::Other);
        assert!(error.to_string().starts_with("invalid PROPFIND response"), "{error}");
    }
}
