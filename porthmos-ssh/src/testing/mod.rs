use std::{
    collections::{HashMap, HashSet, VecDeque},
    ffi::OsString,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use porthmos_vfs::{Answer, Prompter, Question, Target, async_trait};
use russh::{
    Channel, ChannelId, ChannelMsg, ChannelOpenFailure,
    server::{self, Auth, Msg, Server as _, Session as ServerSession},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

use crate::{ConnectOptions, Session, connect};

pub const USER: &str = "u";
pub const PASSWORD: &str = "pw";

const HOST_KEY: &str = include_str!("host_key");

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub hide_scp: bool,
    pub refuse_exec: bool,
    pub max_sessions: Option<usize>,
    pub replace: &'static [(&'static str, &'static str)],
}

#[derive(Clone)]
struct Handler {
    root: PathBuf,
    path: OsString,
    options: Options,
    channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    open: Arc<Mutex<HashSet<ChannelId>>>,
    running: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

impl server::Server for Handler {
    type Handler = Self;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        Self { channels: Arc::default(), open: Arc::default(), ..self.clone() }
    }
}

async fn relay(mut child: tokio::process::Child, channel: Channel<Msg>) -> Option<u32> {
    let (mut read, write) = channel.split();
    let mut stdin = child.stdin.take()?;
    let mut stdout = child.stdout.take()?;
    let mut stderr = child.stderr.take()?;
    let feed = tokio::spawn(async move {
        while let Some(message) = read.wait().await {
            match message {
                ChannelMsg::Data { data } => {
                    let written = stdin.write_all(&data).await;
                    if written.is_err() {
                        break;
                    }
                }
                ChannelMsg::Eof | ChannelMsg::Close => break,
                _ => {}
            }
        }
    });
    let mut broken = false;
    let out = async {
        let mut buffer = vec![0u8; 32 * 1024];
        while let Ok(count) = stdout.read(&mut buffer).await {
            if count == 0 {
                break;
            }
            if write.data(&buffer[..count]).await.is_err() {
                broken = true;
                break;
            }
        }
    };
    let err = async {
        let mut buffer = vec![0u8; 4 * 1024];
        while let Ok(count) = stderr.read(&mut buffer).await {
            if count == 0 || write.extended_data(1, &buffer[..count]).await.is_err() {
                break;
            }
        }
    };
    tokio::join!(out, err);
    if broken {
        let _ = child.start_kill();
    }
    let status = child.wait().await.ok()?.code().unwrap_or(1) as u32;
    feed.abort();
    let _ = write.exit_status(status).await;
    let _ = write.eof().await;
    let _ = write.close().await;
    Some(status)
}

impl server::Handler for Handler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == USER && password == PASSWORD { Auth::Accept } else { Auth::reject() })
    }

    async fn channel_open_session(
        &mut self, channel: Channel<Msg>, reply: server::ChannelOpenHandle, _: &mut ServerSession,
    ) -> Result<(), Self::Error> {
        if self.options.max_sessions.is_some_and(|max| self.open.lock().unwrap().len() >= max) {
            reply.reject(ChannelOpenFailure::ResourceShortage).await;
            return Ok(());
        }
        self.open.lock().unwrap().insert(channel.id());
        self.channels.lock().unwrap().insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn channel_close(&mut self, id: ChannelId, _: &mut ServerSession) -> Result<(), Self::Error> {
        self.channels.lock().unwrap().remove(&id);
        self.open.lock().unwrap().remove(&id);
        Ok(())
    }

    async fn exec_request(
        &mut self, id: ChannelId, data: &[u8], session: &mut ServerSession,
    ) -> Result<(), Self::Error> {
        let Some(channel) = self.channels.lock().unwrap().remove(&id) else {
            return session.channel_failure(id);
        };
        if self.options.refuse_exec {
            self.open.lock().unwrap().remove(&id);
            session.channel_failure(id)?;
            let _ = channel.close().await;
            return Ok(());
        }
        session.channel_success(id)?;
        let requested = String::from_utf8_lossy(data).into_owned();
        let command = self
            .options
            .replace
            .iter()
            .find(|(prefix, _)| requested.starts_with(prefix))
            .map_or(requested.clone(), |(_, replacement)| replacement.to_string());
        let child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&self.root)
            .env("PATH", &self.path)
            .env("HOME", &self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(russh::Error::IO)?;
        let (running, peak, open) = (self.running.clone(), self.peak.clone(), self.open.clone());
        let now = running.fetch_add(1, Ordering::SeqCst) + 1;
        peak.fetch_max(now, Ordering::SeqCst);
        tokio::spawn(async move {
            relay(child, channel).await;
            running.fetch_sub(1, Ordering::SeqCst);
            open.lock().unwrap().remove(&id);
        });
        Ok(())
    }
}

fn search_path(hide_scp: bool) -> OsString {
    let path = std::env::var_os("PATH").unwrap_or_default();
    if !hide_scp {
        return path;
    }
    let tools = tempfile::tempdir().expect("a temporary directory").keep();
    for tool in ["sh", "ls", "cat", "tail", "mkdir", "rm", "mv", "printf", "sleep", "mkfifo", "head", "env"] {
        if let Some(found) = std::env::split_paths(&path).map(|dir| dir.join(tool)).find(|candidate| candidate.exists())
        {
            let _ = std::os::unix::fs::symlink(found, tools.join(tool));
        }
    }
    std::env::join_paths([tools]).unwrap_or_default()
}

pub struct SshServer {
    pub port: u16,
    pub root: tempfile::TempDir,
    known_hosts: tempfile::TempDir,
    peak: Arc<AtomicUsize>,
}

impl SshServer {
    pub async fn start(options: Options) -> Self {
        let root = tempfile::tempdir().expect("a temporary directory");
        let config = Arc::new(server::Config {
            keys: vec![russh::keys::PrivateKey::from_openssh(HOST_KEY).expect("the test host key parses")],
            auth_rejection_time: std::time::Duration::from_millis(1),
            auth_rejection_time_initial: Some(std::time::Duration::from_millis(0)),
            ..Default::default()
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a free port");
        let port = listener.local_addr().expect("a bound address").port();
        let peak = Arc::new(AtomicUsize::new(0));
        let mut handler = Handler {
            root: root.path().to_path_buf(),
            path: search_path(options.hide_scp),
            options,
            channels: Arc::default(),
            open: Arc::default(),
            running: Arc::default(),
            peak: peak.clone(),
        };
        tokio::spawn(async move {
            let _ = handler.run_on_socket(config, &listener).await;
        });
        Self { port, root, known_hosts: tempfile::tempdir().expect("a temporary directory"), peak }
    }

    pub fn known_hosts(&self) -> PathBuf {
        self.known_hosts.path().join("known_hosts")
    }

    pub fn connect_options(&self) -> ConnectOptions {
        ConnectOptions { known_hosts: Some(self.known_hosts()), use_agent: false }
    }

    pub fn target(&self) -> Target {
        Target {
            name: "box".into(),
            host: "127.0.0.1".into(),
            port: self.port,
            username: USER.into(),
            password: Some(PASSWORD.into()),
            options: Default::default(),
        }
    }

    pub async fn session(&self) -> Session {
        connect(&self.target(), &mut answers(vec![Some(Answer::Confirmed)]), &self.connect_options())
            .await
            .expect("the test server accepts the password")
    }

    pub fn peak_commands(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

pub struct Answers {
    pub answers: VecDeque<Option<Answer>>,
    pub asked: Vec<Question>,
}

pub fn answers(answers: Vec<Option<Answer>>) -> Answers {
    Answers { answers: answers.into(), asked: Vec::new() }
}

#[async_trait]
impl Prompter for Answers {
    async fn ask(&mut self, question: Question) -> Option<Answer> {
        self.asked.push(question);
        self.answers.pop_front().flatten()
    }
}
