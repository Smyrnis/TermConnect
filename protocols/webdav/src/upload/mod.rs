use std::{
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, ready},
    time::Duration,
};

use bytes::Bytes;
use futures_util::future::BoxFuture;
use tokio::{
    io::{AsyncWrite, AsyncWriteExt, DuplexStream},
    task::{AbortHandle, JoinHandle},
    time::Sleep,
};

use crate::idle::timed_out;

pub(crate) const PATCH_CHUNK: usize = 4 * 1024 * 1024;
pub(crate) const PIPE_CAPACITY: usize = 256 * 1024;

pub(crate) type Verify = Box<dyn FnOnce(u64) -> BoxFuture<'static, io::Result<()>> + Send>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Waits {
    pub(crate) idle: Duration,
    pub(crate) reply: Duration,
}

pub(crate) type SendChunk = Arc<dyn Fn(u64, Bytes) -> BoxFuture<'static, io::Result<()>> + Send + Sync>;

pub(crate) struct PutUpload {
    pipe: Option<DuplexStream>,
    task: Option<JoinHandle<io::Result<()>>>,
    abort: AbortHandle,
    waits: Waits,
    stalled: Option<Pin<Box<Sleep>>>,
    verify: Option<Verify>,
    finishing: Option<BoxFuture<'static, io::Result<()>>>,
    written: u64,
}

impl PutUpload {
    pub(crate) fn new(pipe: DuplexStream, task: JoinHandle<io::Result<()>>, verify: Verify, waits: Waits) -> Self {
        let abort = task.abort_handle();
        Self {
            pipe: Some(pipe),
            task: Some(task),
            abort,
            waits,
            stalled: None,
            verify: Some(verify),
            finishing: None,
            written: 0,
        }
    }

    fn poll_failure(&mut self, cx: &mut Context<'_>) -> Poll<io::Error> {
        let Some(task) = self.task.as_mut() else {
            return Poll::Ready(io::Error::other("the upload is closed"));
        };
        let outcome = ready!(Pin::new(task).poll(cx));
        self.task = None;
        Poll::Ready(match outcome {
            Ok(Ok(())) => io::Error::other("the server ended the upload early"),
            Ok(Err(err)) => err,
            Err(err) => io::Error::other(err),
        })
    }

    fn finishing(&mut self) -> &mut BoxFuture<'static, io::Result<()>> {
        let pipe = self.pipe.take();
        let task = self.task.take();
        let verify = self.verify.take();
        let written = self.written;
        let reply = self.waits.reply;
        let abort = self.abort.clone();
        self.finishing.get_or_insert_with(|| {
            Box::pin(async move {
                if let Some(mut pipe) = pipe {
                    pipe.shutdown().await?;
                }
                if let Some(task) = task {
                    match tokio::time::timeout(reply, task).await {
                        Ok(joined) => joined.map_err(io::Error::other)??,
                        Err(_) => {
                            abort.abort();
                            return Err(timed_out());
                        }
                    }
                }
                match verify {
                    Some(verify) => verify(written).await,
                    None => Ok(()),
                }
            })
        })
    }
}

impl AsyncWrite for PutUpload {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.task.as_ref().is_some_and(JoinHandle::is_finished) {
            return this.poll_failure(cx).map(Err);
        }
        let Some(pipe) = this.pipe.as_mut() else {
            return Poll::Ready(Err(io::Error::other("the upload is closed")));
        };
        match Pin::new(pipe).poll_write(cx, buf) {
            Poll::Ready(Ok(written)) => {
                this.stalled = None;
                this.written += written as u64;
                Poll::Ready(Ok(written))
            }
            Poll::Ready(Err(_)) => this.poll_failure(cx).map(Err),
            Poll::Pending => {
                let idle = this.waits.idle;
                let deadline = this.stalled.get_or_insert_with(|| Box::pin(tokio::time::sleep(idle)));
                match deadline.as_mut().poll(cx) {
                    Poll::Ready(()) => Poll::Ready(Err(timed_out())),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut().pipe.as_mut() {
            Some(pipe) => Pin::new(pipe).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().finishing().as_mut().poll(cx)
    }
}

impl Drop for PutUpload {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

pub(crate) struct PatchUpload {
    send: SendChunk,
    next: u64,
    chunk: usize,
    buffer: Vec<u8>,
    pending: Option<BoxFuture<'static, io::Result<()>>>,
}

impl PatchUpload {
    pub(crate) fn new(send: SendChunk, start: u64, chunk: usize) -> Self {
        Self { send, next: start, chunk, buffer: Vec::new(), pending: None }
    }

    fn start_chunk(&mut self) {
        let data = Bytes::from(std::mem::take(&mut self.buffer));
        let start = self.next;
        self.next += data.len() as u64;
        self.pending = Some((self.send)(start, data));
    }

    fn poll_pending(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let Some(pending) = self.pending.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        let result = ready!(pending.as_mut().poll(cx));
        self.pending = None;
        Poll::Ready(result)
    }
}

impl AsyncWrite for PatchUpload {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        ready!(this.poll_pending(cx))?;
        let accepted = buf.len().min(this.chunk - this.buffer.len());
        this.buffer.extend_from_slice(&buf[..accepted]);
        if this.buffer.len() == this.chunk {
            this.start_chunk();
        }
        Poll::Ready(Ok(accepted))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().poll_pending(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.poll_pending(cx))?;
        if !this.buffer.is_empty() {
            this.start_chunk();
            ready!(this.poll_pending(cx))?;
        }
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests;
