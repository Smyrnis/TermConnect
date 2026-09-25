#![allow(dead_code)]

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Arc,
};

use porthmos_vfs::{Answer, Prompter, Question, Target, async_trait};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use unftp_core::auth::{AuthenticationError, Authenticator, Credentials, Principal};

pub mod scripted;

pub const USER: &str = "u";

#[derive(Debug)]
struct PasswordAuth(Option<&'static str>);

#[async_trait]
impl Authenticator for PasswordAuth {
    async fn authenticate(&self, username: &str, creds: &Credentials) -> Result<Principal, AuthenticationError> {
        match self.0 {
            None => Ok(Principal { username: username.to_string() }),
            Some(expected) if username == USER && creds.password.as_deref() == Some(expected) => {
                Ok(Principal { username: username.to_string() })
            }
            Some(_) => Err(AuthenticationError::BadPassword),
        }
    }
}

pub struct TlsFiles {
    pub cert: PathBuf,
    pub key: PathBuf,
    pub der: Vec<u8>,
    pub key_der: Vec<u8>,
    _dir: tempfile::TempDir,
}

pub fn self_signed_tls(name: &str) -> TlsFiles {
    let dir = tempfile::tempdir().unwrap();
    let certified = rcgen::generate_simple_self_signed(vec![name.to_string(), "127.0.0.1".to_string()]).unwrap();
    let cert = dir.path().join("cert.pem");
    let key = dir.path().join("key.pem");
    std::fs::write(&cert, certified.cert.pem()).unwrap();
    std::fs::write(&key, certified.signing_key.serialize_pem()).unwrap();
    TlsFiles {
        cert,
        key,
        der: certified.cert.der().to_vec(),
        key_der: certified.signing_key.serialize_der(),
        _dir: dir,
    }
}

pub struct Server {
    pub port: u16,
    pub root: tempfile::TempDir,
}

pub struct Options {
    pub password: Option<&'static str>,
    pub tls: Option<(PathBuf, PathBuf)>,
    pub idle_timeout: Option<u64>,
}

pub async fn start(password: Option<&'static str>, tls: Option<&TlsFiles>) -> Server {
    start_with(Options { password, tls: tls.map(|files| (files.cert.clone(), files.key.clone())), idle_timeout: None })
        .await
}

pub async fn start_with(options: Options) -> Server {
    let root = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let root_path = root.path().to_path_buf();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let root_path = root_path.clone();
            let mut builder = libunftp::ServerBuilder::with_authenticator(
                Box::new(move || unftp_sbe_fs::Filesystem::new(root_path.clone()).unwrap()),
                Arc::new(PasswordAuth(options.password)),
            )
            .passive_host([127, 0, 0, 1]);
            if let Some((cert, key)) = &options.tls {
                builder = builder.ftps(cert.clone(), key.clone());
            }
            if let Some(seconds) = options.idle_timeout {
                builder = builder.idle_session_timeout(seconds);
            }
            let server = builder.build().unwrap();
            tokio::spawn(async move {
                let _ = server.service(stream).await;
            });
        }
    });
    Server { port, root }
}

pub async fn start_implicit(tls: &TlsFiles) -> u16 {
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(tls.der.clone())],
            rustls::pki_types::PrivateKeyDer::try_from(tls.key_der.clone()).unwrap(),
        )
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(stream) = acceptor.accept(stream).await else {
                    return;
                };
                let (reader, mut writer) = tokio::io::split(stream);
                let mut lines = BufReader::new(reader).lines();
                let _ = writer.write_all(b"220 ready\r\n").await;
                while let Ok(Some(line)) = lines.next_line().await {
                    let command = line.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
                    let reply: &[u8] = match command.as_str() {
                        "USER" => b"331 password please\r\n",
                        "PASS" => b"230 logged in\r\n",
                        "TYPE" | "OPTS" => b"200 ok\r\n",
                        "PWD" => b"257 \"/\"\r\n",
                        "FEAT" => b"211-Features:\r\n211 End\r\n",
                        "QUIT" => b"221 bye\r\n",
                        _ => b"502 not implemented\r\n",
                    };
                    let _ = writer.write_all(reply).await;
                    let _ = writer.flush().await;
                }
            });
        }
    });
    port
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

pub fn target(port: u16, username: &str, password: Option<&str>, security: &str) -> Target {
    Target {
        name: "srv".into(),
        host: "127.0.0.1".into(),
        port,
        username: username.into(),
        password: password.map(str::to_string),
        options: [("security".to_string(), security.to_string())].into_iter().collect(),
    }
}

pub fn store_path(dir: &Path) -> PathBuf {
    dir.join("known_certificates.toml")
}
