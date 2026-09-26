use porthmos_vfs::ErrorKind;

use super::*;

#[test]
fn a_file_header_gives_mode_size_and_name() {
    assert_eq!(parse_header(b"C0644 5 a b.txt").unwrap(), Header { mode: 0o644, size: 5, name: "a b.txt".to_string() });
}

#[test]
fn anything_but_a_file_header_is_refused() {
    for line in [&b"D0755 0 dir"[..], b"C0644", b"Cxyz 5 a", b"C0644 big a"] {
        assert_eq!(parse_header(line).unwrap_err().kind(), ErrorKind::Other);
    }
}

#[test]
fn replies_are_ok_warnings_or_errors() {
    assert_eq!(parse_reply(b"\0rest"), Some((Reply::Ok, 1)));
    assert_eq!(parse_reply(b"\x01scp: odd\n"), Some((Reply::Warning("scp: odd".to_string()), 10)));
    assert_eq!(
        parse_reply(b"\x02scp: /x: No such file or directory\n"),
        Some((Reply::Fatal("scp: /x: No such file or directory".to_string()), 36))
    );
    assert_eq!(parse_reply(b"\x02partial"), None);
    assert_eq!(parse_reply(b""), None);
}

#[test]
fn the_sink_header_uses_a_plain_name() {
    assert_eq!(sink_header(12, "report.txt"), "C0644 12 report.txt\n");
    assert_eq!(sink_header(0, "bad\nname"), "C0644 0 bad name\n");
}
