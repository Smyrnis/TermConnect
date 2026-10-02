#![allow(dead_code)]

use std::{
    collections::{HashMap, VecDeque},
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use base64::{Engine, engine::general_purpose::STANDARD};
use dav_server::{DavHandler, body::Body, fakels::FakeLs, localfs::LocalFs};
use http_body_util::BodyExt;
use hyper::{
    Request, Response, StatusCode,
    body::Incoming,
    header::{AUTHORIZATION, CONTENT_LENGTH, HeaderValue, LOCATION, RANGE, TRANSFER_ENCODING, WWW_AUTHENTICATE},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use md5::{Digest, Md5};
use porthmos_vfs::{Answer, Prompter, Question, Target, async_trait};
use tokio::net::TcpListener;

pub const USER: &str = "u";
pub const REALM: &str = "porthmos";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    Open,
    Basic(&'static str),
    BasicForWrites(&'static str),
    Digest(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quirk {
    None,
    RejectPut(u16),
    EmptyPut,
    IgnoreRange,
    RedirectPropfind,
    NotDav,
    NoPartialUpdate,
    RotateNonce,
    HangPropfind,
    HangMove,
    PartialMove,
    ForeignHrefs,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub auth: Auth,
    pub quirk: Quirk,
    pub prefix: &'static str,
}

impl Default for Options {
    fn default() -> Self {
        Self { auth: Auth::Open, quirk: Quirk::None, prefix: "" }
    }
}

pub struct TlsFiles {
    pub der: Vec<u8>,
    pub key_der: Vec<u8>,
}

pub fn self_signed_tls() -> TlsFiles {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()]).unwrap();
    TlsFiles { der: certified.cert.der().to_vec(), key_der: certified.signing_key.serialize_der() }
}

pub struct Server {
    pub port: u16,
    pub root: tempfile::TempDir,
}

struct State {
    handler: DavHandler,
    options: Options,
    generation: AtomicU64,
    authenticated: AtomicU64,
}

fn acceptor(tls: &TlsFiles) -> tokio_rustls::TlsAcceptor {
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(tls.der.clone())],
            rustls::pki_types::PrivateKeyDer::try_from(tls.key_der.clone()).unwrap(),
        )
        .unwrap();
    tokio_rustls::TlsAcceptor::from(Arc::new(config))
}

pub async fn start(options: Options) -> Server {
    start_with_tls(options, None).await
}

pub async fn start_with_tls(options: Options, tls: Option<&TlsFiles>) -> Server {
    let root = tempfile::tempdir().unwrap();
    let mut builder =
        DavHandler::builder().filesystem(LocalFs::new(root.path(), false, false, false)).locksystem(FakeLs::new());
    if !options.prefix.is_empty() {
        builder = builder.strip_prefix(options.prefix);
    }
    let state = Arc::new(State {
        handler: builder.build_handler(),
        options,
        generation: AtomicU64::new(0),
        authenticated: AtomicU64::new(0),
    });
    let acceptor = tls.map(acceptor);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let state = state.clone();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request| {
                    let state = state.clone();
                    async move { Ok::<_, Infallible>(serve(&state, request).await) }
                });
                match acceptor {
                    Some(acceptor) => {
                        if let Ok(stream) = acceptor.accept(stream).await {
                            let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), service).await;
                        }
                    }
                    None => {
                        let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), service).await;
                    }
                }
            });
        }
    });
    Server { port, root }
}

fn plain(status: StatusCode, body: &'static str) -> Response<Body> {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response
}

async fn serve(state: &State, mut request: Request<Incoming>) -> Response<Body> {
    if let Some(challenge) = unauthorized(state, &request) {
        return challenge;
    }
    if !request.uri().path().starts_with(state.options.prefix) {
        return plain(StatusCode::NOT_FOUND, "");
    }
    match (state.options.quirk, request.method().as_str()) {
        (Quirk::NotDav, _) => return plain(StatusCode::OK, "<html>hello</html>"),
        (Quirk::RedirectPropfind, "PROPFIND") => {
            let mut response = plain(StatusCode::MOVED_PERMANENTLY, "");
            response.headers_mut().insert(LOCATION, HeaderValue::from_static("https://elsewhere.example/dav/"));
            return response;
        }
        (Quirk::RejectPut(code), "PUT") => return plain(StatusCode::from_u16(code).unwrap(), ""),
        (Quirk::EmptyPut, "PUT") => {
            let (mut parts, _) = request.into_parts();
            parts.headers.remove(CONTENT_LENGTH);
            parts.headers.remove(TRANSFER_ENCODING);
            return state.handler.handle(Request::from_parts(parts, Body::empty())).await;
        }
        (Quirk::IgnoreRange, "GET") => {
            request.headers_mut().remove(RANGE);
        }
        (Quirk::HangPropfind, "PROPFIND") | (Quirk::HangMove, "MOVE") => std::future::pending::<()>().await,
        (Quirk::PartialMove, "MOVE") => {
            let mut response = plain(
                StatusCode::MULTI_STATUS,
                r#"<?xml version="1.0" encoding="utf-8"?><D:multistatus xmlns:D="DAV:"><D:response><D:href>/d/locked.txt</D:href><D:status>HTTP/1.1 423 Locked</D:status></D:response></D:multistatus>"#,
            );
            response.headers_mut().insert(hyper::header::CONTENT_TYPE, HeaderValue::from_static("application/xml"));
            return response;
        }
        _ => {}
    }
    let listing = request.method().as_str() == "PROPFIND"
        && request.headers().get("depth").is_some_and(|depth| depth.as_bytes() == b"1");
    let mut response = state.handler.handle(request).await;
    if state.options.quirk == Quirk::ForeignHrefs && listing {
        response = foreign_hrefs(response).await;
    }
    if state.options.quirk == Quirk::NoPartialUpdate && response.headers().contains_key("dav") {
        response.headers_mut().insert("dav", HeaderValue::from_static("1,2"));
    }
    response
}

async fn foreign_hrefs(response: Response<Body>) -> Response<Body> {
    let (mut parts, body) = response.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    let rewritten = String::from_utf8_lossy(&bytes).replace("<D:href>/", "<D:href>/elsewhere/");
    parts.headers.remove(CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(rewritten))
}

fn challenge(value: String) -> Response<Body> {
    let mut response = plain(StatusCode::UNAUTHORIZED, "");
    response.headers_mut().insert(WWW_AUTHENTICATE, HeaderValue::from_str(&value).unwrap());
    response
}

fn digest_challenge(nonce: &str, stale: bool) -> String {
    let stale = if stale { ", stale=true" } else { "" };
    format!(r#"Digest realm="{REALM}", qop="auth", algorithm=MD5, nonce="{nonce}"{stale}"#)
}

fn md5_hex(text: &str) -> String {
    format!("{:x}", Md5::digest(text.as_bytes()))
}

fn digest_fields(header: &str) -> Option<HashMap<String, String>> {
    let rest = header.strip_prefix("Digest ")?;
    Some(
        rest.split(", ")
            .filter_map(|item| item.split_once('='))
            .map(|(key, value)| (key.trim().to_string(), value.trim().trim_matches('"').to_string()))
            .collect(),
    )
}

fn digest_valid(fields: &HashMap<String, String>, method: &str, target: &str, password: &str) -> bool {
    let field = |name: &str| fields.get(name).map(String::as_str).unwrap_or_default();
    let ha1 = md5_hex(&format!("{}:{REALM}:{password}", field("username")));
    let ha2 = md5_hex(&format!("{method}:{}", field("uri")));
    let expected =
        md5_hex(&format!("{ha1}:{}:{}:{}:{}:{ha2}", field("nonce"), field("nc"), field("cnonce"), field("qop")));
    field("username") == USER && field("uri") == target && field("response") == expected
}

fn unauthorized(state: &State, request: &Request<Incoming>) -> Option<Response<Body>> {
    let given = request.headers().get(AUTHORIZATION).and_then(|value| value.to_str().ok());
    match state.options.auth {
        Auth::Open => None,
        Auth::Basic(password) => {
            let expected = format!("Basic {}", STANDARD.encode(format!("{USER}:{password}")));
            (given != Some(expected.as_str())).then(|| challenge(format!(r#"Basic realm="{REALM}""#)))
        }
        Auth::BasicForWrites(password) => {
            let reading = matches!(request.method().as_str(), "GET" | "HEAD" | "PROPFIND" | "OPTIONS");
            let expected = format!("Basic {}", STANDARD.encode(format!("{USER}:{password}")));
            (!reading && given != Some(expected.as_str())).then(|| challenge(format!(r#"Basic realm="{REALM}""#)))
        }
        Auth::Digest(password) => {
            let target = request.uri().path_and_query().map(|value| value.as_str()).unwrap_or("/").to_string();
            let nonce = format!("nonce{}", state.generation.load(Ordering::SeqCst));
            match given.and_then(digest_fields) {
                Some(fields) if digest_valid(&fields, request.method().as_str(), &target, password) => {
                    if fields.get("nonce") != Some(&nonce) {
                        return Some(challenge(digest_challenge(&nonce, true)));
                    }
                    let served = state.authenticated.fetch_add(1, Ordering::SeqCst) + 1;
                    if state.options.quirk == Quirk::RotateNonce && served.is_multiple_of(3) {
                        state.generation.fetch_add(1, Ordering::SeqCst);
                    }
                    None
                }
                _ => Some(challenge(digest_challenge(&nonce, false))),
            }
        }
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

pub fn target(port: u16, username: &str, password: Option<&str>) -> Target {
    Target {
        name: "srv".into(),
        host: "127.0.0.1".into(),
        port,
        username: username.into(),
        password: password.map(str::to_string),
        options: [("security".to_string(), "http".to_string())].into_iter().collect(),
    }
}

pub fn with_option(mut target: Target, key: &str, value: &str) -> Target {
    target.options.insert(key.to_string(), value.to_string());
    target
}
