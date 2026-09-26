use std::path::Path;

use porthmos_vfs::ErrorKind;

use super::*;

#[test]
fn listing_commands_use_utc_and_the_c_locale() {
    let env = "TZ=UTC0 LC_ALL=C QUOTING_STYLE=literal TIME_STYLE=locale";
    assert_eq!(list(Path::new("/srv/a b")), format!("{env} ls -lanL -- '/srv/a b/'"));
    assert_eq!(list(Path::new("/")), format!("{env} ls -lanL -- '/'"));
    assert_eq!(stat(Path::new("/it's")), format!(r"{env} ls -ldnL -- '/it'\''s'"));
}

#[test]
fn operations_quote_every_path_after_a_double_dash() {
    assert_eq!(mkdir(Path::new("/-x")), "mkdir -- '/-x'");
    assert_eq!(remove(Path::new("/$HOME")), "rm -- '/$HOME'");
    assert_eq!(remove_tree(Path::new("/`id`")), "rm -rf -- '/`id`'");
    assert_eq!(rename(Path::new("/a"), Path::new("/b c")), "mv -f -- '/a' '/b c'");
}

#[test]
fn transfer_commands() {
    assert_eq!(read_from(Path::new("/f"), 0), "cat -- '/f'");
    assert_eq!(read_from(Path::new("/f"), 10), "tail -c +11 -- '/f'");
    assert_eq!(scp_source(Path::new("/f")), "scp -f -- '/f'");
    assert_eq!(scp_sink(Path::new("/f")), ": > '/f' && scp -t -- '/f'");
    assert_eq!(append(Path::new("/f")), "cat >> '/f'");
    assert_eq!(create(Path::new("/f")), "cat > '/f'");
}

fn mapped(stderr: &str) -> (ErrorKind, String) {
    let error = failure(stderr.as_bytes(), Some(1), Path::new("/x"), "ls");
    (error.kind(), error.to_string())
}

#[test]
fn stderr_maps_to_error_kinds() {
    assert_eq!(
        mapped("ls: cannot access '/x': No such file or directory\n"),
        (ErrorKind::NotFound, "/x not found".to_string())
    );
    for text in
        ["rm: cannot remove '/x': Permission denied", "mv: Operation not permitted", "mkdir: Read-only file system"]
    {
        assert_eq!(mapped(text), (ErrorKind::PermissionDenied, "/x: permission denied".to_string()));
    }
    assert_eq!(
        mapped("mkdir: cannot create directory '/x': File exists"),
        (ErrorKind::Other, "/x already exists".to_string())
    );
    assert_eq!(mapped("something odd\nsecond line"), (ErrorKind::Other, "something odd".to_string()));
    assert_eq!(mapped(""), (ErrorKind::Other, "ls failed (exit 1)".to_string()));
}

#[test]
fn the_probe_marks_a_working_shell() {
    assert!(PROBE.starts_with("printf 'porthmos"));
}
