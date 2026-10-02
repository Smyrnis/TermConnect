use std::sync::Mutex;

use tokio::io::AsyncWriteExt;

use super::*;

const MIB: usize = 1024 * 1024;

type Sent = Arc<Mutex<Vec<(u32, usize)>>>;

fn recording(failure: Option<&'static str>) -> (SendPart, Sent) {
    let sent: Sent = Arc::default();
    let log = sent.clone();
    let send: SendPart = Arc::new(move |number, data: Bytes| {
        log.lock().unwrap().push((number, data.len()));
        Box::pin(async move {
            match failure {
                Some(message) => Err(io::Error::other(message)),
                None => Ok(()),
            }
        })
    });
    (send, sent)
}

fn finished() -> (Finish, Arc<Mutex<u32>>) {
    let calls: Arc<Mutex<u32>> = Arc::default();
    let count = calls.clone();
    let finish: Finish = Box::new(move || {
        *count.lock().unwrap() += 1;
        Box::pin(async { Ok(()) })
    });
    (finish, calls)
}

#[test]
fn every_part_has_the_same_size() {
    assert_eq!(PART_SIZE, 16 * MIB);
}

#[tokio::test]
async fn full_parts_are_sent_in_order_and_the_rest_at_shutdown() {
    let (send, sent) = recording(None);
    let mut writer = MultipartWriter::new(send, 1, None, "too large".to_string());

    writer.write_all(&vec![7u8; 33 * MIB]).await.unwrap();
    writer.shutdown().await.unwrap();

    assert_eq!(*sent.lock().unwrap(), vec![(1, 16 * MIB), (2, 16 * MIB), (3, MIB)]);
}

#[tokio::test]
async fn a_continued_upload_numbers_parts_from_where_it_stopped() {
    let (send, sent) = recording(None);
    let mut writer = MultipartWriter::new(send, 4, None, "too large".to_string());

    writer.write_all(b"tail").await.unwrap();
    writer.shutdown().await.unwrap();

    assert_eq!(*sent.lock().unwrap(), vec![(4, 4)]);
}

#[tokio::test]
async fn an_empty_new_upload_sends_one_empty_part() {
    let (send, sent) = recording(None);

    MultipartWriter::new(send, 1, None, "too large".to_string()).shutdown().await.unwrap();

    assert_eq!(*sent.lock().unwrap(), vec![(1, 0)]);
}

#[tokio::test]
async fn an_empty_continued_upload_sends_nothing() {
    let (send, sent) = recording(None);

    MultipartWriter::new(send, 3, None, "too large".to_string()).shutdown().await.unwrap();

    assert!(sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_finish_step_runs_once_after_the_last_part() {
    let (send, sent) = recording(None);
    let (finish, calls) = finished();
    let mut writer = MultipartWriter::new(send, 1, Some(finish), "too large".to_string());

    writer.write_all(b"abc").await.unwrap();
    writer.shutdown().await.unwrap();

    assert_eq!(*sent.lock().unwrap(), vec![(1, 3)]);
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn a_rejected_part_fails_the_upload() {
    let (send, _) = recording(Some("SlowDown"));
    let mut writer = MultipartWriter::new(send, 1, None, "too large".to_string());

    let error = async {
        writer.write_all(&vec![0u8; 17 * MIB]).await?;
        writer.shutdown().await
    }
    .await
    .unwrap_err();

    assert_eq!(error.to_string(), "SlowDown");
}

#[tokio::test]
async fn more_than_ten_thousand_parts_is_too_large() {
    let (send, sent) = recording(None);
    let mut writer = MultipartWriter::new(send, MAX_PARTS + 1, None, "/big.bin is too large".to_string());

    writer.write_all(b"x").await.unwrap();

    assert_eq!(writer.shutdown().await.unwrap_err().to_string(), "/big.bin is too large");
    assert!(sent.lock().unwrap().is_empty());
}
