use std::{
    future::Future,
    io,
    path::PathBuf,
    pin::Pin,
    task::{Context, Poll, ready},
    time::Duration,
};

use porthmos_ssh::{ExecChannel, Output};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf},
    sync::OwnedSemaphorePermit,
    task::{AbortHandle, JoinHandle},
    time::{Instant, Sleep},
};

use crate::{
    commands::failure,
    wire::{Reply, parse_reply},
};

pub(crate) const NO_RESPONSE: &str = "the server did not respond in time";
pub(crate) const SIZE_CHANGED: &str = "the source changed size during the upload";
const EARLY_END: &str = "the server ended the transfer early";
const CHUNK: usize = 64 * 1024;
const SEND_PIECE: usize = 16 * 1024;

pub(crate) type Finishing = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;
pub(crate) type Finisher = Box<dyn FnOnce(ExecChannel) -> Finishing + Send>;

pub(crate) fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, NO_RESPONSE)
}

pub(crate) fn size_changed() -> io::Error {
    io::Error::other(SIZE_CHANGED)
}

pub(crate) async fn within<T>(limit: Duration, work: impl Future<Output = io::Result<T>>) -> io::Result<T> {
    tokio::time::timeout(limit, work).await.map_err(|_| timed_out())?
}

pub(crate) fn checked_exit(output: Output, path: &std::path::Path, command: &str) -> io::Result<()> {
    if output.status == Some(0) {
        Ok(())
    } else {
        Err(io::Error::other(failure(&output.stderr, output.status, path, command)))
    }
}

pub(crate) async fn read_line(output: &mut DuplexStream) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    loop {
        let byte = output.read_u8().await.map_err(|_| io::Error::other(EARLY_END))?;
        if byte == b'\n' {
            return Ok(line);
        }
        line.push(byte);
    }
}

pub(crate) async fn read_reply(output: &mut DuplexStream, path: &std::path::Path) -> io::Result<()> {
    let first = output.read_u8().await.map_err(|_| io::Error::other(EARLY_END))?;
    if first == 0 {
        return Ok(());
    }
    let mut bytes = vec![first];
    bytes.extend(read_line(output).await?);
    bytes.push(b'\n');
    match parse_reply(&bytes) {
        Some((Reply::Ok, _)) => Ok(()),
        Some((Reply::Warning(message) | Reply::Fatal(message), _)) => {
            Err(io::Error::other(failure(message.as_bytes(), None, path, "scp")))
        }
        None => Err(io::Error::other(EARLY_END)),
    }
}

struct Watch {
    idle: Duration,
    deadline: Pin<Box<Sleep>>,
}

impl Watch {
    fn new(idle: Duration) -> Self {
        Self { idle, deadline: Box::pin(tokio::time::sleep(idle)) }
    }

    fn progress(&mut self) {
        let idle = self.idle;
        self.deadline.as_mut().reset(Instant::now() + idle);
    }

    fn expired(&mut self, cx: &mut Context<'_>) -> bool {
        self.deadline.as_mut().poll(cx).is_ready()
    }
}

pub(crate) struct ChannelReader {
    channel: Option<ExecChannel>,
    remaining: Option<u64>,
    finisher: Option<Finisher>,
    finishing: Option<Finishing>,
    done: bool,
    watch: Watch,
    scratch: Vec<u8>,
    _permit: OwnedSemaphorePermit,
}

impl ChannelReader {
    pub(crate) fn new(
        channel: ExecChannel, remaining: Option<u64>, finisher: Finisher, idle: Duration, permit: OwnedSemaphorePermit,
    ) -> Self {
        Self {
            channel: Some(channel),
            remaining,
            finisher: Some(finisher),
            finishing: None,
            done: false,
            watch: Watch::new(idle),
            scratch: vec![0u8; CHUNK],
            _permit: permit,
        }
    }

    fn start_finishing(&mut self) {
        if let (Some(channel), Some(finisher)) = (self.channel.take(), self.finisher.take()) {
            self.finishing = Some(finisher(channel));
        }
    }
}

impl AsyncRead for ChannelReader {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        loop {
            if this.done {
                return Poll::Ready(Ok(()));
            }
            if let Some(finishing) = this.finishing.as_mut() {
                let result = ready!(finishing.as_mut().poll(cx));
                this.finishing = None;
                this.done = true;
                return Poll::Ready(result);
            }
            if this.remaining == Some(0) {
                this.start_finishing();
                continue;
            }
            let Some(channel) = this.channel.as_mut() else {
                this.done = true;
                continue;
            };
            let limit = this.remaining.map_or(buf.remaining(), |left| buf.remaining().min(left as usize)).min(CHUNK);
            let mut scratch = ReadBuf::new(&mut this.scratch[..limit]);
            match Pin::new(&mut channel.output).poll_read(cx, &mut scratch) {
                Poll::Ready(Ok(())) => {
                    let count = scratch.filled().len();
                    if count == 0 {
                        if this.remaining.is_some() {
                            return Poll::Ready(Err(io::Error::other(EARLY_END)));
                        }
                        this.start_finishing();
                        continue;
                    }
                    buf.put_slice(scratch.filled());
                    if let Some(left) = this.remaining.as_mut() {
                        *left -= count as u64;
                    }
                    this.watch.progress();
                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending if this.watch.expired(cx) => return Poll::Ready(Err(timed_out())),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

pub(crate) struct Pump {
    pub(crate) channel: ExecChannel,
    pub(crate) expected: Option<u64>,
    pub(crate) path: PathBuf,
    pub(crate) command: String,
    pub(crate) idle: Duration,
}

async fn pump(mut job: Pump, mut body: DuplexStream) -> io::Result<()> {
    let mut buffer = vec![0u8; CHUNK];
    let mut sent = 0u64;
    loop {
        let count = body.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        for piece in buffer[..count].chunks(SEND_PIECE) {
            if let Err(err) = within(job.idle, job.channel.input.send(piece)).await {
                if err.kind() == io::ErrorKind::TimedOut {
                    return Err(err);
                }
                let output = within(job.idle, async { Ok(job.channel.finish().await) }).await?;
                checked_exit(output, &job.path, &job.command)?;
                return Err(io::Error::other(EARLY_END));
            }
        }
        sent += count as u64;
    }
    if let Some(size) = job.expected {
        if sent != size {
            let _ = job.channel.input.close().await;
            let _ = job.channel.finish().await;
            return Err(size_changed());
        }
        job.channel.input.send(&[0]).await?;
        read_reply(&mut job.channel.output, &job.path).await?;
    }
    job.channel.input.close().await?;
    let output = job.channel.finish().await;
    checked_exit(output, &job.path, &job.command)
}

pub(crate) struct ChannelWriter {
    pipe: Option<DuplexStream>,
    task: Option<JoinHandle<io::Result<()>>>,
    abort: AbortHandle,
    limit: Option<u64>,
    written: u64,
    final_wait: Duration,
    finishing: Option<Finishing>,
    _permit: OwnedSemaphorePermit,
}

impl ChannelWriter {
    pub(crate) fn new(job: Pump, final_wait: Duration, permit: OwnedSemaphorePermit) -> Self {
        let (pipe, body) = tokio::io::duplex(CHUNK * 4);
        let limit = job.expected;
        let task = tokio::spawn(pump(job, body));
        let abort = task.abort_handle();
        Self {
            pipe: Some(pipe),
            task: Some(task),
            abort,
            limit,
            written: 0,
            final_wait,
            finishing: None,
            _permit: permit,
        }
    }

    fn poll_failure(&mut self, cx: &mut Context<'_>) -> Poll<io::Error> {
        let Some(task) = self.task.as_mut() else {
            return Poll::Ready(io::Error::other(EARLY_END));
        };
        let outcome = ready!(Pin::new(task).poll(cx));
        self.task = None;
        Poll::Ready(match outcome {
            Ok(Ok(())) => io::Error::other(EARLY_END),
            Ok(Err(err)) => err,
            Err(err) => io::Error::other(err),
        })
    }
}

impl AsyncWrite for ChannelWriter {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.limit.is_some_and(|limit| this.written + buf.len() as u64 > limit) {
            return Poll::Ready(Err(size_changed()));
        }
        if this.task.as_ref().is_some_and(JoinHandle::is_finished) {
            return this.poll_failure(cx).map(Err);
        }
        let Some(pipe) = this.pipe.as_mut() else {
            return Poll::Ready(Err(io::Error::other(EARLY_END)));
        };
        match Pin::new(pipe).poll_write(cx, buf) {
            Poll::Ready(Ok(count)) => {
                this.written += count as u64;
                Poll::Ready(Ok(count))
            }
            Poll::Ready(Err(_)) => this.poll_failure(cx).map(Err),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut().pipe.as_mut() {
            Some(pipe) => Pin::new(pipe).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.finishing.is_none() {
            let (pipe, task, final_wait, abort) =
                (this.pipe.take(), this.task.take(), this.final_wait, this.abort.clone());
            this.finishing = Some(Box::pin(async move {
                if let Some(mut pipe) = pipe {
                    pipe.shutdown().await?;
                }
                let finished = match task {
                    Some(task) => within(final_wait, async { task.await.map_err(io::Error::other)? }).await,
                    None => Ok(()),
                };
                if finished.as_ref().is_err_and(|err| err.kind() == io::ErrorKind::TimedOut) {
                    abort.abort();
                }
                finished
            }));
        }
        let Some(finishing) = this.finishing.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        finishing.as_mut().poll(cx)
    }
}

impl Drop for ChannelWriter {
    fn drop(&mut self) {
        if self.finishing.is_none() {
            self.abort.abort();
        }
    }
}

#[cfg(test)]
mod tests;
