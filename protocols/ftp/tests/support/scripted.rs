use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

#[derive(Clone, Debug)]
pub enum Node {
    File(Vec<u8>),
    Dir,
    Link(String),
}

#[derive(Clone, Default)]
pub struct Script {
    pub refuse_rest: bool,
    pub refuse_append: bool,
    pub silent_after_login: bool,
    pub login_reply: Option<&'static str>,
    pub silent_from_start: bool,
    pub mdtm_reply: Option<&'static str>,
    pub listing_delay: Option<Duration>,
}

#[derive(Clone)]
pub struct Scripted {
    pub port: u16,
    pub tree: Arc<Mutex<BTreeMap<String, Node>>>,
    pub connections: Arc<AtomicUsize>,
    pub passwords: Arc<Mutex<Vec<String>>>,
}

impl Scripted {
    pub fn file(&self, path: &str, data: &[u8]) {
        self.tree.lock().unwrap().insert(path.to_string(), Node::File(data.to_vec()));
    }

    pub fn dir(&self, path: &str) {
        self.tree.lock().unwrap().insert(path.to_string(), Node::Dir);
    }

    pub fn link(&self, path: &str, target: &str) {
        self.tree.lock().unwrap().insert(path.to_string(), Node::Link(target.to_string()));
    }

    pub fn connection_count(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    pub fn get(&self, path: &str) -> Option<Node> {
        self.tree.lock().unwrap().get(path).cloned()
    }
}

fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) => "/",
        Some(index) => &path[..index],
        None => "/",
    }
}

fn absolute(cwd: &str, path: &str) -> String {
    if path.starts_with('/') {
        path.trim_end_matches('/').to_string().max("/".to_string())
    } else if cwd == "/" {
        format!("/{path}")
    } else {
        format!("{cwd}/{path}")
    }
}

fn resolves_to_dir(tree: &BTreeMap<String, Node>, path: &str) -> bool {
    match tree.get(path) {
        Some(Node::Dir) => true,
        Some(Node::Link(target)) => matches!(tree.get(target), Some(Node::Dir)),
        _ => path == "/",
    }
}

fn list_lines(tree: &BTreeMap<String, Node>, dir: &str) -> String {
    let mut out = String::new();
    for (path, node) in tree.iter().filter(|(path, _)| parent_of(path) == dir && path.as_str() != "/") {
        let name = path.rsplit('/').next().unwrap_or_default();
        let line = match node {
            Node::File(data) => format!("-rw-r--r--    1 u        u        {:>8} Mar  1 12:34 {name}", data.len()),
            Node::Dir => format!("drwxr-xr-x    2 u        u               0 Mar  1 12:34 {name}"),
            Node::Link(target) => format!("lrwxrwxrwx    1 u        u               7 Mar  1 12:34 {name} -> {target}"),
        };
        out.push_str(&line);
        out.push_str("\r\n");
    }
    out
}

pub async fn start(script: Script) -> Scripted {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let tree: Arc<Mutex<BTreeMap<String, Node>>> = Arc::default();
    let connections: Arc<AtomicUsize> = Arc::default();
    let passwords: Arc<Mutex<Vec<String>>> = Arc::default();
    let scripted =
        Scripted { port, tree: tree.clone(), connections: connections.clone(), passwords: passwords.clone() };
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            connections.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(serve(stream, tree.clone(), script.clone(), passwords.clone()));
        }
    });
    scripted
}

fn record_password(
    passwords: &Mutex<Vec<String>>, password: &str, reply: Option<&'static str>,
) -> Option<&'static str> {
    passwords.lock().unwrap().push(password.to_string());
    reply
}

async fn serve(
    stream: TcpStream, tree: Arc<Mutex<BTreeMap<String, Node>>>, script: Script, passwords: Arc<Mutex<Vec<String>>>,
) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut cwd = "/".to_string();
    let mut passive: Option<TcpListener> = None;
    let mut restart: u64 = 0;
    let mut rename_from: Option<String> = None;
    let mut logged_in = false;
    let _ = writer.write_all(b"220 scripted\r\n").await;
    while let Ok(Some(line)) = lines.next_line().await {
        let (command, argument) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let command = command.to_ascii_uppercase();
        if script.silent_from_start {
            continue;
        }
        if logged_in && script.silent_after_login && !matches!(command.as_str(), "TYPE" | "OPTS" | "FEAT" | "PWD") {
            continue;
        }
        let reply = match command.as_str() {
            "USER" => "331 password please".to_string(),
            "PASS" => match record_password(&passwords, argument, script.login_reply) {
                Some(reply) => reply.to_string(),
                None => {
                    logged_in = true;
                    "230 logged in".to_string()
                }
            },
            "TYPE" | "OPTS" => "200 ok".to_string(),
            "FEAT" => "211 no features".to_string(),
            "PWD" => format!("257 \"{cwd}\""),
            "NOOP" => "200 ok".to_string(),
            "CWD" => {
                let target = absolute(&cwd, argument);
                if resolves_to_dir(&tree.lock().unwrap(), &target) {
                    cwd = target;
                    "250 ok".to_string()
                } else {
                    "550 No such file or directory".to_string()
                }
            }
            "PASV" => {
                let data = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = data.local_addr().unwrap().port();
                passive = Some(data);
                format!("227 Entering Passive Mode (127,0,0,1,{},{})", port / 256, port % 256)
            }
            "REST" if script.refuse_rest => "502 REST not supported".to_string(),
            "REST" => {
                restart = argument.parse().unwrap_or(0);
                "350 restarting".to_string()
            }
            "SIZE" => match tree.lock().unwrap().get(&absolute(&cwd, argument)) {
                Some(Node::File(data)) => format!("213 {}", data.len()),
                _ => "550 No such file or directory".to_string(),
            },
            "MDTM" => match tree.lock().unwrap().get(&absolute(&cwd, argument)) {
                Some(Node::File(_)) => script.mdtm_reply.unwrap_or("213 20260301123400").to_string(),
                _ => "550 No such file or directory".to_string(),
            },
            "MKD" => {
                tree.lock().unwrap().insert(absolute(&cwd, argument), Node::Dir);
                "257 created".to_string()
            }
            "DELE" => {
                let target = absolute(&cwd, argument);
                let mut tree = tree.lock().unwrap();
                match tree.get(&target) {
                    Some(Node::File(_)) | Some(Node::Link(_)) => {
                        tree.remove(&target);
                        "250 deleted".to_string()
                    }
                    _ => "550 Not a plain file".to_string(),
                }
            }
            "RMD" => {
                let target = absolute(&cwd, argument);
                let mut tree = tree.lock().unwrap();
                let has_children = tree.keys().any(|path| parent_of(path) == target && *path != target);
                match tree.get(&target) {
                    Some(Node::Dir) if !has_children => {
                        tree.remove(&target);
                        "250 removed".to_string()
                    }
                    _ => "550 Directory not empty or not a directory".to_string(),
                }
            }
            "RNFR" => {
                let from = absolute(&cwd, argument);
                if tree.lock().unwrap().contains_key(&from) {
                    rename_from = Some(from);
                    "350 ready".to_string()
                } else {
                    "550 No such file or directory".to_string()
                }
            }
            "RNTO" => match rename_from.take() {
                Some(from) => {
                    let mut tree = tree.lock().unwrap();
                    let node = tree.remove(&from).unwrap();
                    tree.insert(absolute(&cwd, argument), node);
                    "250 renamed".to_string()
                }
                None => "503 RNFR first".to_string(),
            },
            "LIST" | "RETR" | "STOR" | "APPE" => {
                if command == "APPE" && script.refuse_append {
                    let _ = writer.write_all(b"502 APPE not supported\r\n").await;
                    continue;
                }
                let Some(data_listener) = passive.take() else {
                    let _ = writer.write_all(b"425 use PASV first\r\n").await;
                    continue;
                };
                let target = absolute(&cwd, if argument.is_empty() { "." } else { argument });
                let target = if argument.is_empty() || argument == "." { cwd.clone() } else { target };
                let _ = writer.write_all(b"150 opening data connection\r\n").await;
                let (mut data, _) = data_listener.accept().await.unwrap();

                match command.as_str() {
                    "LIST" => {
                        let listing = list_lines(&tree.lock().unwrap(), &target);
                        for line in listing.split_inclusive("\r\n") {
                            if let Some(delay) = script.listing_delay {
                                tokio::time::sleep(delay).await;
                            }
                            let _ = data.write_all(line.as_bytes()).await;
                        }
                    }
                    "RETR" => {
                        let content = match tree.lock().unwrap().get(&target) {
                            Some(Node::File(content)) => content[restart as usize..].to_vec(),
                            _ => Vec::new(),
                        };
                        let _ = data.write_all(&content).await;
                    }
                    _ => {
                        let mut received = Vec::new();
                        let _ = data.read_to_end(&mut received).await;
                        let mut tree = tree.lock().unwrap();
                        let existing = match tree.get(&target) {
                            Some(Node::File(content)) => content.clone(),
                            _ => Vec::new(),
                        };
                        let mut content = match command.as_str() {
                            "APPE" => existing,
                            _ => existing[..(restart as usize).min(existing.len())].to_vec(),
                        };
                        content.extend_from_slice(&received);
                        tree.insert(target, Node::File(content));
                    }
                }
                let _ = data.shutdown().await;
                drop(data);
                restart = 0;
                "226 transfer complete".to_string()
            }
            "QUIT" => {
                let _ = writer.write_all(b"221 bye\r\n").await;
                return;
            }
            _ => "502 not implemented".to_string(),
        };
        let _ = writer.write_all(format!("{reply}\r\n").as_bytes()).await;
    }
}
