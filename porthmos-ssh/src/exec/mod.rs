use std::io;

use anyhow::anyhow;
use porthmos_vfs::{ErrorKind, ProtocolError};
use russh::{ChannelMsg, ChannelWriteHalf, client::Msg};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream},
    sync::watch,
    task::JoinHandle,
};

use crate::Session;

const OUTPUT_BUFFER: usize = 256 * 1024;
const STDERR: u32 = 1;
const CLOSED: &str = "the command's channel is closed";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: Option<u32>,
}

pub struct ExecInput {
    write: Option<ChannelWriteHalf<Msg>>,
    closed: watch::Receiver<bool>,
}

fn closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, CLOSED)
}

impl ExecInput {
    pub async fn send(&self, data: &[u8]) -> io::Result<()> {
        let write = self.write.as_ref().ok_or_else(closed)?;
        let mut ended = self.closed.clone();
        if *ended.borrow() {
            return Err(closed());
        }
        tokio::select! {
            result = write.data(data) => result.map_err(io::Error::other),
            _ = ended.wait_for(|ended| *ended) => Err(closed()),
        }
    }

    pub async fn close(&self) -> io::Result<()> {
        let write = self.write.as_ref().ok_or_else(closed)?;
        write.eof().await.map_err(io::Error::other)
    }
}

pub struct ExecChannel {
    pub input: ExecInput,
    pub output: DuplexStream,
    done: Option<JoinHandle<Output>>,
    finished: bool,
}

impl ExecChannel {
    pub async fn finish(mut self) -> Output {
        drop(std::mem::replace(&mut self.output, tokio::io::duplex(1).1));
        let output = match self.done.take() {
            Some(done) => done.await.unwrap_or_default(),
            None => Output::default(),
        };
        self.finished = true;
        output
    }
}

impl Drop for ExecChannel {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let (Some(write), Ok(runtime)) = (self.input.write.take(), tokio::runtime::Handle::try_current()) {
            runtime.spawn(async move {
                let _ = write.close().await;
            });
        }
    }
}

fn session_error(err: russh::Error) -> ProtocolError {
    ProtocolError::new(ErrorKind::Other, anyhow!(err))
}

pub async fn open_exec(session: &Session, command: &str) -> Result<ExecChannel, ProtocolError> {
    let channel = session.handle().channel_open_session().await.map_err(session_error)?;
    channel.exec(true, command).await.map_err(session_error)?;
    let (mut read, write) = channel.split();
    let (mut sink, output) = tokio::io::duplex(OUTPUT_BUFFER);
    let (ended, closed) = watch::channel(false);
    let done = tokio::spawn(async move {
        let mut finished = Output::default();
        let mut forwarding = true;
        while let Some(message) = read.wait().await {
            match message {
                ChannelMsg::Data { data } if forwarding => forwarding = sink.write_all(&data).await.is_ok(),
                ChannelMsg::ExtendedData { data, ext } if ext == STDERR => finished.stderr.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status } => finished.status = Some(exit_status),
                ChannelMsg::Failure | ChannelMsg::Close => break,
                _ => {}
            }
        }
        let _ = ended.send(true);
        finished
    });
    Ok(ExecChannel { input: ExecInput { write: Some(write), closed }, output, done: Some(done), finished: false })
}

pub async fn exec(session: &Session, command: &str) -> Result<Output, ProtocolError> {
    let mut channel = open_exec(session, command).await?;
    channel.input.close().await.map_err(|err| ProtocolError::new(ErrorKind::Other, err))?;
    let mut stdout = Vec::new();
    channel.output.read_to_end(&mut stdout).await.map_err(|err| ProtocolError::new(ErrorKind::Other, err))?;
    let mut output = channel.finish().await;
    if output.status.is_none() {
        return Err(ProtocolError::new(ErrorKind::Other, anyhow!("the server did not run the command")));
    }
    output.stdout = stdout;
    Ok(output)
}
