use std::path::Path;

use tokio::io::AsyncWriteExt;

use super::*;

async fn stream(bytes: &[u8]) -> DuplexStream {
    let (mut sender, receiver) = tokio::io::duplex(1024);
    sender.write_all(bytes).await.unwrap();
    drop(sender);
    receiver
}

#[tokio::test]
async fn a_zero_byte_is_a_positive_reply() {
    assert!(read_reply(&mut stream(b"\0").await, Path::new("/f")).await.is_ok());
}

#[tokio::test]
async fn warnings_and_errors_become_failures_with_their_text() {
    let warning = read_reply(&mut stream(b"\x01scp: odd\n").await, Path::new("/f")).await.unwrap_err();
    let fatal =
        read_reply(&mut stream(b"\x02scp: /f: No such file or directory\n").await, Path::new("/f")).await.unwrap_err();

    assert_eq!(warning.to_string(), "scp: odd");
    assert_eq!(fatal.to_string(), "/f not found");
}

#[tokio::test]
async fn a_stream_ending_early_is_reported() {
    assert_eq!(
        read_reply(&mut stream(b"").await, Path::new("/f")).await.unwrap_err().to_string(),
        "the server ended the transfer early"
    );
    assert_eq!(
        read_line(&mut stream(b"C0644 3").await).await.unwrap_err().to_string(),
        "the server ended the transfer early"
    );
}

#[tokio::test]
async fn lines_end_at_a_newline() {
    let mut output = stream(b"C0644 3 f\nabc").await;

    assert_eq!(read_line(&mut output).await.unwrap(), b"C0644 3 f");
}
