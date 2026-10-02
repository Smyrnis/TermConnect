use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, ready},
};

use bytes::Bytes;
use futures_util::future::BoxFuture;
use tokio::io::AsyncWrite;

pub(crate) const MAX_PARTS: u32 = 10_000;
pub(crate) const PART_SIZE: usize = 16 * 1024 * 1024;

pub(crate) type SendPart = Arc<dyn Fn(u32, Bytes) -> BoxFuture<'static, io::Result<()>> + Send + Sync>;
pub(crate) type Finish = Box<dyn FnOnce() -> BoxFuture<'static, io::Result<()>> + Send>;

pub(crate) struct MultipartWriter {
    send: SendPart,
    next: u32,
    fresh: bool,
    sent: bool,
    buffer: Vec<u8>,
    pending: Option<BoxFuture<'static, io::Result<()>>>,
    finish: Option<Finish>,
    finishing: Option<BoxFuture<'static, io::Result<()>>>,
    too_large: String,
}

impl MultipartWriter {
    pub(crate) fn new(send: SendPart, first_part: u32, finish: Option<Finish>, too_large: String) -> Self {
        Self {
            send,
            next: first_part,
            fresh: first_part == 1,
            sent: false,
            buffer: Vec::new(),
            pending: None,
            finish,
            finishing: None,
            too_large,
        }
    }

    fn start_part(&mut self) -> io::Result<()> {
        if self.next > MAX_PARTS {
            return Err(io::Error::other(self.too_large.clone()));
        }
        let data = Bytes::from(std::mem::take(&mut self.buffer));
        self.pending = Some((self.send)(self.next, data));
        self.next += 1;
        self.sent = true;
        Ok(())
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

impl AsyncWrite for MultipartWriter {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        ready!(this.poll_pending(cx))?;
        let accepted = buf.len().min(PART_SIZE - this.buffer.len());
        this.buffer.extend_from_slice(&buf[..accepted]);
        if this.buffer.len() == PART_SIZE {
            this.start_part()?;
        }
        Poll::Ready(Ok(accepted))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().poll_pending(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.finishing.is_none() {
            ready!(this.poll_pending(cx))?;
            if !this.buffer.is_empty() || (this.fresh && !this.sent) {
                this.start_part()?;
                ready!(this.poll_pending(cx))?;
            }
            match this.finish.take() {
                Some(finish) => this.finishing = Some(finish()),
                None => return Poll::Ready(Ok(())),
            }
        }
        let Some(finishing) = this.finishing.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        let result = ready!(finishing.as_mut().poll(cx));
        this.finishing = None;
        Poll::Ready(result)
    }
}

#[cfg(test)]
mod tests;
