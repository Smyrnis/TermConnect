use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

#[tokio::test(start_paused = true)]
async fn data_arriving_within_the_idle_time_is_read() {
    let (mut sender, receiver) = tokio::io::duplex(64);
    let mut reader = IdleReader::new(receiver, Duration::from_secs(5));
    tokio::spawn(async move {
        for _ in 0..3 {
            tokio::time::sleep(Duration::from_secs(4)).await;
            sender.write_all(b"ab").await.unwrap();
        }
    });

    let mut data = Vec::new();
    reader.read_to_end(&mut data).await.unwrap();

    assert_eq!(data, b"ababab");
}

#[tokio::test(start_paused = true)]
async fn a_stream_silent_for_the_idle_time_times_out() {
    let (_sender, receiver) = tokio::io::duplex(64);
    let mut reader = IdleReader::new(receiver, Duration::from_secs(5));

    let error = reader.read_u8().await.unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(error.to_string(), "the server did not respond in time");
}
