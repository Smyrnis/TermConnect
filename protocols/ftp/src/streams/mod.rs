use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, ready},
};

use futures_util::future::BoxFuture;
use suppaftp::{
    FtpResult,
    tokio::{AsyncRustlsStream, TransferStream},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::{errors::ftp_error, pool::Pool, session::Connection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteStart {
    RestartStore,
    Append,
    Store,
}

impl WriteStart {
    pub(crate) fn offset(self, requested: u64) -> u64 {
        match self {
            WriteStart::RestartStore | WriteStart::Append => requested,
            WriteStart::Store => 0,
        }
    }
}

pub(crate) fn first_write_start(offset: u64) -> WriteStart {
    if offset == 0 { WriteStart::Store } else { WriteStart::RestartStore }
}

pub(crate) fn fallback(refused: WriteStart) -> Option<WriteStart> {
    match refused {
        WriteStart::RestartStore => Some(WriteStart::Append),
        WriteStart::Append => Some(WriteStart::Store),
        WriteStart::Store => None,
    }
}

pub(crate) fn ends_data(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::UnexpectedEof
}

type Transfer = TransferStream<AsyncRustlsStream>;
type Finishing = BoxFuture<'static, (FtpResult<()>, Connection)>;

enum State {
    Streaming(Box<Transfer>, Box<Connection>),
    Finishing(Finishing),
    Done,
}

struct Finisher {
    state: State,
    pool: Arc<Pool>,
}

impl Finisher {
    fn new(stream: Transfer, connection: Connection, pool: Arc<Pool>) -> Self {
        Self { state: State::Streaming(Box::new(stream), Box::new(connection)), pool }
    }

    fn start_finishing(&mut self) {
        if !matches!(self.state, State::Streaming(..)) {
            return;
        }
        if let State::Streaming(stream, connection) = std::mem::replace(&mut self.state, State::Done) {
            self.state = State::Finishing(Box::pin(async move { (stream.finish().await, *connection) }));
        }
    }

    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.start_finishing();
        let State::Finishing(future) = &mut self.state else {
            return Poll::Ready(Ok(()));
        };
        let (result, connection) = ready!(future.as_mut().poll(cx));
        self.state = State::Done;
        match result {
            Ok(()) => {
                self.pool.give_back(connection);
                Poll::Ready(Ok(()))
            }
            Err(err) => Poll::Ready(Err(io::Error::other(ftp_error(err).to_string()))),
        }
    }
}

pub(crate) struct FtpReader(Finisher);

impl FtpReader {
    pub(crate) fn new(stream: Transfer, connection: Connection, pool: Arc<Pool>) -> Self {
        Self(Finisher::new(stream, connection, pool))
    }
}

impl AsyncRead for FtpReader {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let finisher = &mut self.0;
        if let State::Streaming(stream, _) = &mut finisher.state {
            let before = buf.filled().len();
            match ready!(Pin::new(stream.as_mut()).poll_read(cx, buf)) {
                Err(err) if !ends_data(&err) => return Poll::Ready(Err(err)),
                Err(_) => {}
                Ok(()) if buf.filled().len() > before || buf.remaining() == 0 => return Poll::Ready(Ok(())),
                Ok(()) => {}
            }
        }
        finisher.poll_finish(cx)
    }
}

pub(crate) struct FtpWriter(Finisher);

impl FtpWriter {
    pub(crate) fn new(stream: Transfer, connection: Connection, pool: Arc<Pool>) -> Self {
        Self(Finisher::new(stream, connection, pool))
    }
}

fn finished_error() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "the upload is already finished")
}

impl AsyncWrite for FtpWriter {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match &mut self.0.state {
            State::Streaming(stream, _) => Pin::new(stream.as_mut()).poll_write(cx, buf),
            _ => Poll::Ready(Err(finished_error())),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.0.state {
            State::Streaming(stream, _) => Pin::new(stream.as_mut()).poll_flush(cx),
            _ => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.0.poll_finish(cx)
    }
}

#[cfg(test)]
mod tests;
