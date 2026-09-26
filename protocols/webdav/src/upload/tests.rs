use std::sync::Mutex;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

const WAITS: Waits = Waits { idle: std::time::Duration::from_secs(5), reply: std::time::Duration::from_secs(60) };

type Sent = Arc<Mutex<Vec<(u64, Vec<u8>)>>>;

fn recording(failure: Option<&'static str>) -> (SendChunk, Sent) {
    let sent: Sent = Arc::default();
    let log = sent.clone();
    let send: SendChunk = Arc::new(move |start, data: Bytes| {
        log.lock().unwrap().push((start, data.to_vec()));
        Box::pin(async move {
            match failure {
                Some(message) => Err(io::Error::other(message)),
                None => Ok(()),
            }
        })
    });
    (send, sent)
}

fn verified_count() -> (Verify, Arc<Mutex<Option<u64>>>) {
    let seen: Arc<Mutex<Option<u64>>> = Arc::default();
    let record = seen.clone();
    let verify: Verify = Box::new(move |written| {
        *record.lock().unwrap() = Some(written);
        Box::pin(async { Ok(()) })
    });
    (verify, seen)
}

#[tokio::test]
async fn patch_sends_full_chunks_then_the_rest_at_their_offsets() {
    let (send, sent) = recording(None);
    let mut upload = PatchUpload::new(send, 100, 4);

    upload.write_all(b"abcdefghij").await.unwrap();
    upload.shutdown().await.unwrap();

    assert_eq!(*sent.lock().unwrap(), vec![(100, b"abcd".to_vec()), (104, b"efgh".to_vec()), (108, b"ij".to_vec())]);
}

#[tokio::test]
async fn patch_with_nothing_written_sends_nothing() {
    let (send, sent) = recording(None);

    PatchUpload::new(send, 0, 4).shutdown().await.unwrap();

    assert!(sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_rejected_patch_chunk_fails_the_upload() {
    let (send, _) = recording(Some("insufficient storage on the server"));
    let mut upload = PatchUpload::new(send, 0, 4);

    let error = async {
        upload.write_all(b"abcdefghij").await?;
        upload.shutdown().await
    }
    .await
    .unwrap_err();

    assert_eq!(error.to_string(), "insufficient storage on the server");
}

#[tokio::test]
async fn put_streams_everything_then_verifies_the_count() {
    let (pipe, mut body) = tokio::io::duplex(8);
    let task = tokio::spawn(async move {
        let mut received = Vec::new();
        body.read_to_end(&mut received).await?;
        assert_eq!(received, b"hello world");
        Ok(())
    });
    let (verify, seen) = verified_count();
    let mut upload = PutUpload::new(pipe, task, verify, WAITS);

    upload.write_all(b"hello world").await.unwrap();
    upload.shutdown().await.unwrap();

    assert_eq!(*seen.lock().unwrap(), Some(11));
}

#[tokio::test]
async fn a_put_the_server_ends_early_fails_the_next_write() {
    let (pipe, body) = tokio::io::duplex(8);
    let task = tokio::spawn(async move {
        drop(body);
        Err(io::Error::other("insufficient storage on the server"))
    });
    let (verify, _) = verified_count();
    let mut upload = PutUpload::new(pipe, task, verify, WAITS);

    let error = upload.write_all(&[0u8; 64]).await.unwrap_err();

    assert_eq!(error.to_string(), "insufficient storage on the server");
}

#[tokio::test]
async fn a_put_rejected_after_the_body_fails_at_shutdown_without_verifying() {
    let (pipe, mut body) = tokio::io::duplex(64);
    let task = tokio::spawn(async move {
        let mut received = Vec::new();
        body.read_to_end(&mut received).await?;
        Err(io::Error::other("/a.txt: permission denied"))
    });
    let (verify, seen) = verified_count();
    let mut upload = PutUpload::new(pipe, task, verify, WAITS);

    upload.write_all(b"abc").await.unwrap();

    assert_eq!(upload.shutdown().await.unwrap_err().to_string(), "/a.txt: permission denied");
    assert_eq!(*seen.lock().unwrap(), None);
}

#[tokio::test]
async fn a_failed_verification_fails_the_shutdown() {
    let (pipe, mut body) = tokio::io::duplex(64);
    let task = tokio::spawn(async move {
        let mut received = Vec::new();
        body.read_to_end(&mut received).await?;
        Ok(())
    });
    let verify: Verify = Box::new(|written| {
        Box::pin(async move { Err(io::Error::other(format!("the server stored 0 of {written} bytes"))) })
    });
    let mut upload = PutUpload::new(pipe, task, verify, WAITS);

    upload.write_all(b"abcde").await.unwrap();

    assert_eq!(upload.shutdown().await.unwrap_err().to_string(), "the server stored 0 of 5 bytes");
}

struct DropFlag(Arc<Mutex<bool>>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        *self.0.lock().unwrap() = true;
    }
}

#[tokio::test]
async fn dropping_a_put_while_it_shuts_down_aborts_the_request() {
    let dropped: Arc<Mutex<bool>> = Arc::default();
    let flag = DropFlag(dropped.clone());
    let (pipe, _body) = tokio::io::duplex(64);
    let task = tokio::spawn(async move {
        let _flag = flag;
        std::future::pending::<()>().await;
        Ok(())
    });
    let (verify, _) = verified_count();
    let mut upload = PutUpload::new(pipe, task, verify, WAITS);
    assert!(tokio::time::timeout(std::time::Duration::from_millis(20), upload.shutdown()).await.is_err());

    drop(upload);
    tokio::task::yield_now().await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    assert!(*dropped.lock().unwrap());
}

#[tokio::test(start_paused = true)]
async fn a_put_the_server_stops_reading_times_out() {
    let (pipe, body) = tokio::io::duplex(8);
    let task = tokio::spawn(async move {
        let _body = body;
        std::future::pending::<()>().await;
        Ok(())
    });
    let (verify, _) = verified_count();
    let mut upload = PutUpload::new(pipe, task, verify, WAITS);

    let error = upload.write_all(&[0u8; 64]).await.unwrap_err();

    assert_eq!(
        (error.kind(), error.to_string().as_str()),
        (io::ErrorKind::TimedOut, "the server did not respond in time")
    );
}

#[tokio::test(start_paused = true)]
async fn a_put_whose_reply_never_comes_times_out_at_shutdown() {
    let (pipe, mut body) = tokio::io::duplex(64);
    let task = tokio::spawn(async move {
        let mut received = Vec::new();
        body.read_to_end(&mut received).await?;
        std::future::pending::<()>().await;
        Ok(())
    });
    let (verify, seen) = verified_count();
    let mut upload = PutUpload::new(pipe, task, verify, WAITS);
    upload.write_all(b"abc").await.unwrap();

    let error = upload.shutdown().await.unwrap_err();

    assert_eq!(
        (error.kind(), error.to_string().as_str()),
        (io::ErrorKind::TimedOut, "the server did not respond in time")
    );
    assert_eq!(*seen.lock().unwrap(), None);
}
