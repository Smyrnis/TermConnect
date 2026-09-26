use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::{
    Answer, ErrorKind, FileKind, FileSystem, Prompter, Protocol, Question, SearchEvent, SearchQuery, Target,
    walk_search,
};

#[tokio::test]
async fn stat_follows_symlinks_and_read_dir_does_not() {
    let fs = FakeFs::new();
    fs.file("/data/a.txt", b"hello", Some(10)).symlink("/data/link", "/data/a.txt");

    assert_eq!(fs.stat(Path::new("/data/link")).await.unwrap().size, 5);
    let items = fs.read_dir(Path::new("/data")).await.unwrap();
    let link = items.iter().find(|item| item.name == "link").unwrap();
    assert_eq!(link.metadata.kind, FileKind::Symlink);
}

#[tokio::test]
async fn open_write_at_an_offset_truncates_then_appends() {
    let fs = FakeFs::new();
    fs.file("/f.part", b"abcdef", None);
    let mut writer = fs.open_write(Path::new("/f.part"), 3).await.unwrap();
    assert_eq!(writer.offset, 3);
    writer.stream.write_all(b"XY").await.unwrap();
    writer.stream.shutdown().await.unwrap();
    assert_eq!(fs.contents("/f.part").unwrap(), b"abcXY");
}

#[tokio::test]
async fn open_write_on_a_missing_file_starts_at_zero() {
    let fs = FakeFs::new();
    fs.dir("/d");
    let writer = fs.open_write(Path::new("/d/new"), 5).await.unwrap();
    assert_eq!(writer.offset, 0);
}

#[tokio::test]
async fn open_read_starts_at_the_offset() {
    let fs = FakeFs::new();
    fs.file("/f", b"abcdef", None);
    let mut reader = fs.open_read(Path::new("/f"), 2).await.unwrap();
    let mut read = Vec::new();
    reader.read_to_end(&mut read).await.unwrap();
    assert_eq!(read, b"cdef");
}

#[tokio::test]
async fn a_failing_read_errors_while_copying() {
    let fs = FakeFs::new();
    fs.file("/f", b"abc", None).fail_reads("/f");
    let mut reader = fs.open_read(Path::new("/f"), 0).await.unwrap();
    let mut read = Vec::new();
    assert!(reader.read_to_end(&mut read).await.is_err());
}

#[tokio::test]
async fn rename_replaces_an_existing_target() {
    let fs = FakeFs::new();
    fs.file("/a", b"new", None).file("/b", b"old", None);
    fs.rename(Path::new("/a"), Path::new("/b")).await.unwrap();
    assert_eq!(fs.contents("/b").unwrap(), b"new");
    assert!(!fs.exists("/a"));
}

#[tokio::test]
async fn delete_removes_a_directory_tree_without_following_symlinks() {
    let fs = FakeFs::new();
    fs.file("/keep/file", b"x", None).file("/gone/inner/file", b"y", None).symlink("/gone/link", "/keep");
    fs.delete(Path::new("/gone")).await.unwrap();
    assert!(!fs.exists("/gone/inner/file"));
    assert!(fs.exists("/keep/file"));
}

#[tokio::test]
async fn a_failing_read_dir_reports_permission_denied() {
    let fs = FakeFs::new();
    fs.dir("/locked").fail_read_dir("/locked");
    let error = fs.read_dir(Path::new("/locked")).await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::PermissionDenied);
}

#[tokio::test]
async fn list_resolves_symlinked_directories_as_directories() {
    let fs = FakeFs::new();
    fs.dir("/root/real").symlink("/root/alias", "/root/real");
    let entries = fs.list(Path::new("/root")).await.unwrap();
    assert!(entries.iter().all(|entry| entry.is_dir));
}

#[tokio::test]
async fn walk_does_not_descend_into_symlinked_directories() {
    let fs = FakeFs::new();
    fs.file("/root/real/match.txt", b"", None).symlink("/root/alias", "/root/real");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    walk_search(&fs, SearchQuery::new("/root".into(), "*match*".into()), tx, Default::default()).await;
    let mut found = Vec::new();
    while let Ok(SearchEvent::Found(entry)) = rx.try_recv() {
        found.push(entry.path);
    }
    assert_eq!(found, vec![std::path::PathBuf::from("/root/real/match.txt")]);
}

struct Scripted(Option<Answer>);

#[async_trait::async_trait]
impl Prompter for Scripted {
    async fn ask(&mut self, _question: Question) -> Option<Answer> {
        self.0.take()
    }
}

fn target(password: Option<&str>) -> Target {
    Target {
        name: "srv".into(),
        host: "h".into(),
        port: 2222,
        username: "u".into(),
        password: password.map(str::to_string),
        options: Default::default(),
    }
}

#[tokio::test]
async fn fake_protocol_asks_for_a_missing_password_and_reports_cancel() {
    let protocol = FakeProtocol::new(FakeFs::new()).requiring_password("s3cret");
    let error = protocol.connect(&target(None), &mut Scripted(None)).await.err().unwrap();
    assert_eq!(error.kind(), ErrorKind::Cancelled);
}

#[tokio::test]
async fn fake_protocol_rejects_a_wrong_password() {
    let protocol = FakeProtocol::new(FakeFs::new()).requiring_password("s3cret");
    let answer = Some(Answer::Password("nope".into()));
    let error = protocol.connect(&target(None), &mut Scripted(answer)).await.err().unwrap();
    assert_eq!(error.kind(), ErrorKind::AuthRejected);
}

#[tokio::test]
async fn fake_protocol_accepts_a_saved_password_without_asking() {
    let protocol = FakeProtocol::new(FakeFs::new()).requiring_password("s3cret");
    assert!(protocol.connect(&target(Some("s3cret")), &mut Scripted(None)).await.is_ok());
}

#[tokio::test]
async fn a_failing_home_reports_permission_denied() {
    let fs = FakeFs::new();
    fs.fail_home();
    let error = fs.home().await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::PermissionDenied);
}

#[test]
fn a_fake_protocol_can_take_another_id_and_form() {
    let mut form = crate::ConnectionForm::standard(21);
    form.host.label = "Server";
    let protocol = FakeProtocol::new(FakeFs::new()).with_id("ftp").with_form(form.clone());

    assert_eq!(protocol.id(), "ftp");
    assert_eq!(protocol.connection_form(), form);
}

#[tokio::test]
async fn a_writer_on_a_failing_shutdown_path_errors_on_shutdown() {
    let fs = FakeFs::new();
    fs.fail_shutdown("/f");
    let mut writer = fs.open_write(Path::new("/f"), 0).await.unwrap();
    writer.stream.write_all(b"abc").await.unwrap();
    assert!(writer.stream.shutdown().await.is_err());
}

#[tokio::test]
async fn a_sized_write_writes_normally_and_records_the_size() {
    let fs = FakeFs::new();
    fs.dir("/d");

    let mut writer = fs.open_write_sized(Path::new("/d/f"), 0, 3).await.unwrap();
    writer.stream.write_all(b"abc").await.unwrap();
    writer.stream.shutdown().await.unwrap();

    assert_eq!(fs.contents("/d/f").unwrap(), b"abc");
    assert_eq!(fs.written_sizes(), vec![(std::path::PathBuf::from("/d/f"), 3)]);
}

struct OnlyWrites(FakeFs);

#[async_trait::async_trait]
impl FileSystem for OnlyWrites {
    async fn list(&self, dir: &Path) -> Result<Vec<crate::Entry>, crate::ProtocolError> {
        self.0.list(dir).await
    }
    async fn read_dir(&self, dir: &Path) -> Result<Vec<crate::DirItem>, crate::ProtocolError> {
        self.0.read_dir(dir).await
    }
    async fn stat(&self, path: &Path) -> Result<crate::Metadata, crate::ProtocolError> {
        self.0.stat(path).await
    }
    async fn create_dir(&self, path: &Path) -> Result<(), crate::ProtocolError> {
        self.0.create_dir(path).await
    }
    async fn rename(&self, from: &Path, to: &Path) -> Result<(), crate::ProtocolError> {
        self.0.rename(from, to).await
    }
    async fn remove_file(&self, path: &Path) -> Result<(), crate::ProtocolError> {
        self.0.remove_file(path).await
    }
    async fn delete(&self, path: &Path) -> Result<(), crate::ProtocolError> {
        self.0.delete(path).await
    }
    async fn home(&self) -> Result<std::path::PathBuf, crate::ProtocolError> {
        self.0.home().await
    }
    async fn open_read(&self, path: &Path, offset: u64) -> Result<crate::Reader, crate::ProtocolError> {
        self.0.open_read(path, offset).await
    }
    async fn open_write(&self, path: &Path, offset: u64) -> Result<crate::Writer, crate::ProtocolError> {
        self.0.open_write(path, offset).await
    }
}

#[tokio::test]
async fn by_default_a_sized_write_is_a_plain_write() {
    let fs = FakeFs::new();
    fs.dir("/d");
    let plain = OnlyWrites(fs.clone());

    let mut writer = plain.open_write_sized(Path::new("/d/f"), 0, 99).await.unwrap();
    writer.stream.write_all(b"xy").await.unwrap();
    writer.stream.shutdown().await.unwrap();

    assert_eq!(fs.contents("/d/f").unwrap(), b"xy");
    assert!(fs.written_sizes().is_empty());
}

#[test]
fn by_default_a_file_system_has_no_transfer_limit() {
    assert_eq!(OnlyWrites(FakeFs::new()).transfer_limit(), None);
}
