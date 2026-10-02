use std::{
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use tokio::{
    io::{AsyncRead, ReadBuf},
    time::{Instant, Sleep},
};

pub(crate) use crate::errors::NO_RESPONSE;

pub(crate) fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, NO_RESPONSE)
}

pub(crate) struct IdleReader<R> {
    inner: R,
    idle: Duration,
    deadline: Pin<Box<Sleep>>,
}

impl<R> IdleReader<R> {
    pub(crate) fn new(inner: R, idle: Duration) -> Self {
        Self { inner, idle, deadline: Box::pin(tokio::time::sleep(idle)) }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for IdleReader<R> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(result) => {
                let idle = this.idle;
                this.deadline.as_mut().reset(Instant::now() + idle);
                Poll::Ready(result)
            }
            Poll::Pending => match this.deadline.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(Err(timed_out())),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

#[cfg(test)]
mod tests;
