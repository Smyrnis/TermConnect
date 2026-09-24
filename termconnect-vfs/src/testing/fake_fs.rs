use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::{DirItem, Entry, ErrorKind, FileKind, FileSystem, Metadata, ProtocolError, Reader, Writer};

#[derive(Debug, Clone)]
enum Node {
    File { data: Vec<u8>, modified: Option<u64> },
    Dir,
    Symlink(PathBuf),
}

#[derive(Default)]
struct Tree {
    nodes: BTreeMap<PathBuf, Node>,
    failing_read_dirs: BTreeSet<PathBuf>,
    failing_reads: BTreeSet<PathBuf>,
}

const MAX_SYMLINK_HOPS: usize = 8;

impl Tree {
    fn insert_ancestors(&mut self, path: &Path) {
        for ancestor in path.ancestors().skip(1) {
            self.nodes.entry(ancestor.to_path_buf()).or_insert(Node::Dir);
        }
    }

    fn resolve(&self, path: &Path) -> Option<&Node> {
        let mut current = path.to_path_buf();
        for _ in 0..MAX_SYMLINK_HOPS {
            match self.nodes.get(&current)? {
                Node::Symlink(target) => current = target.clone(),
                node => return Some(node),
            }
        }
        None
    }

    fn children(&self, dir: &Path) -> Vec<(PathBuf, &Node)> {
        self.nodes
            .iter()
            .filter(|(path, _)| path.parent() == Some(dir))
            .map(|(path, node)| (path.clone(), node))
            .collect()
    }

    fn is_dir(&self, path: &Path) -> bool {
        matches!(self.nodes.get(path), Some(Node::Dir))
    }
}

fn metadata_of(node: &Node) -> Metadata {
    match node {
        Node::File { data, modified } => {
            Metadata { size: data.len() as u64, modified: *modified, kind: FileKind::File, permissions: None }
        }
        Node::Dir => Metadata { size: 0, modified: None, kind: FileKind::Dir, permissions: None },
        Node::Symlink(_) => Metadata { size: 0, modified: None, kind: FileKind::Symlink, permissions: None },
    }
}

fn not_found(path: &Path) -> ProtocolError {
    ProtocolError::new(ErrorKind::NotFound, anyhow::anyhow!("No such file: {}", path.display()))
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

#[derive(Clone)]
pub struct FakeFs {
    tree: Arc<Mutex<Tree>>,
    resume_backoff: u64,
    home: PathBuf,
}

impl Default for FakeFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeFs {
    pub fn new() -> Self {
        let fs = Self { tree: Arc::default(), resume_backoff: 0, home: PathBuf::from("/home/user") };
        fs.dir("/home/user");
        fs
    }

    pub fn dir(&self, path: impl AsRef<Path>) -> &Self {
        let mut tree = self.tree.lock().unwrap();
        tree.insert_ancestors(path.as_ref());
        tree.nodes.insert(path.as_ref().to_path_buf(), Node::Dir);
        self
    }

    pub fn file(&self, path: impl AsRef<Path>, data: &[u8], modified: Option<u64>) -> &Self {
        let mut tree = self.tree.lock().unwrap();
        tree.insert_ancestors(path.as_ref());
        tree.nodes.insert(path.as_ref().to_path_buf(), Node::File { data: data.to_vec(), modified });
        self
    }

    pub fn symlink(&self, path: impl AsRef<Path>, target: impl AsRef<Path>) -> &Self {
        let mut tree = self.tree.lock().unwrap();
        tree.insert_ancestors(path.as_ref());
        tree.nodes.insert(path.as_ref().to_path_buf(), Node::Symlink(target.as_ref().to_path_buf()));
        self
    }

    pub fn fail_read_dir(&self, path: impl AsRef<Path>) -> &Self {
        self.tree.lock().unwrap().failing_read_dirs.insert(path.as_ref().to_path_buf());
        self
    }

    pub fn fail_reads(&self, path: impl AsRef<Path>) -> &Self {
        self.tree.lock().unwrap().failing_reads.insert(path.as_ref().to_path_buf());
        self
    }

    pub fn with_resume_backoff(mut self, bytes: u64) -> Self {
        self.resume_backoff = bytes;
        self
    }

    pub fn with_home(mut self, home: impl AsRef<Path>) -> Self {
        self.dir(home.as_ref());
        self.home = home.as_ref().to_path_buf();
        self
    }

    pub fn contents(&self, path: impl AsRef<Path>) -> Option<Vec<u8>> {
        match self.tree.lock().unwrap().nodes.get(path.as_ref()) {
            Some(Node::File { data, .. }) => Some(data.clone()),
            _ => None,
        }
    }

    pub fn exists(&self, path: impl AsRef<Path>) -> bool {
        self.tree.lock().unwrap().nodes.contains_key(path.as_ref())
    }
}

struct FailingReader;

impl AsyncRead for FailingReader {
    fn poll_read(self: Pin<&mut Self>, _cx: &mut Context<'_>, _buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::other("injected read failure")))
    }
}

struct FakeWriter {
    tree: Arc<Mutex<Tree>>,
    path: PathBuf,
}

impl AsyncWrite for FakeWriter {
    fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let mut tree = self.tree.lock().unwrap();
        match tree.nodes.get_mut(&self.path) {
            Some(Node::File { data, .. }) => {
                data.extend_from_slice(buf);
                Poll::Ready(Ok(buf.len()))
            }
            _ => Poll::Ready(Err(io::Error::from(io::ErrorKind::NotFound))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[async_trait]
impl FileSystem for FakeFs {
    async fn list(&self, dir: &Path) -> Result<Vec<Entry>, ProtocolError> {
        let tree = self.tree.lock().unwrap();
        if !matches!(tree.resolve(dir), Some(Node::Dir)) {
            return Err(not_found(dir));
        }
        Ok(tree
            .children(dir)
            .into_iter()
            .map(|(path, node)| {
                let resolved = tree.resolve(&path).map(metadata_of).unwrap_or_else(|| metadata_of(node));
                Entry { name: name_of(&path), is_dir: resolved.is_dir(), size: resolved.size, permissions: None, path }
            })
            .collect())
    }

    async fn read_dir(&self, dir: &Path) -> Result<Vec<DirItem>, ProtocolError> {
        let tree = self.tree.lock().unwrap();
        if tree.failing_read_dirs.contains(dir) {
            return Err(ProtocolError::new(ErrorKind::PermissionDenied, anyhow::anyhow!("Permission denied")));
        }
        if !tree.is_dir(dir) {
            return Err(not_found(dir));
        }
        Ok(tree
            .children(dir)
            .into_iter()
            .map(|(path, node)| DirItem { name: name_of(&path), metadata: metadata_of(node), path })
            .collect())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata, ProtocolError> {
        let tree = self.tree.lock().unwrap();
        tree.resolve(path).map(metadata_of).ok_or_else(|| not_found(path))
    }

    async fn create_dir(&self, path: &Path) -> Result<(), ProtocolError> {
        let mut tree = self.tree.lock().unwrap();
        if !path.parent().is_some_and(|parent| tree.is_dir(parent)) {
            return Err(not_found(path));
        }
        if tree.nodes.contains_key(path) {
            return Err(ProtocolError::new(ErrorKind::Other, anyhow::anyhow!("File exists")));
        }
        tree.nodes.insert(path.to_path_buf(), Node::Dir);
        Ok(())
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<(), ProtocolError> {
        let mut tree = self.tree.lock().unwrap();
        if !tree.nodes.contains_key(from) {
            return Err(not_found(from));
        }
        let moved: Vec<(PathBuf, Node)> = tree
            .nodes
            .iter()
            .filter(|(path, _)| path.starts_with(from))
            .map(|(path, node)| (to.join(path.strip_prefix(from).unwrap_or(path)), node.clone()))
            .collect();
        tree.nodes.retain(|path, _| !path.starts_with(from) && !path.starts_with(to));
        tree.nodes.extend(moved);
        Ok(())
    }

    async fn remove_file(&self, path: &Path) -> Result<(), ProtocolError> {
        let mut tree = self.tree.lock().unwrap();
        match tree.nodes.get(path) {
            Some(Node::File { .. } | Node::Symlink(_)) => {
                tree.nodes.remove(path);
                Ok(())
            }
            Some(Node::Dir) => Err(ProtocolError::new(ErrorKind::Other, anyhow::anyhow!("Is a directory"))),
            None => Err(not_found(path)),
        }
    }

    async fn delete(&self, path: &Path) -> Result<(), ProtocolError> {
        let mut tree = self.tree.lock().unwrap();
        if !tree.nodes.contains_key(path) {
            return Err(not_found(path));
        }
        tree.nodes.retain(|candidate, _| !candidate.starts_with(path));
        Ok(())
    }

    async fn home(&self) -> Result<PathBuf, ProtocolError> {
        Ok(self.home.clone())
    }

    async fn open_read(&self, path: &Path, offset: u64) -> Result<Reader, ProtocolError> {
        let tree = self.tree.lock().unwrap();
        if tree.failing_reads.contains(path) {
            return Ok(Box::new(FailingReader));
        }
        match tree.resolve(path) {
            Some(Node::File { data, .. }) => {
                let start = (offset as usize).min(data.len());
                Ok(Box::new(std::io::Cursor::new(data[start..].to_vec())))
            }
            _ => Err(not_found(path)),
        }
    }

    async fn open_write(&self, path: &Path, offset: u64) -> Result<Writer, ProtocolError> {
        let mut tree = self.tree.lock().unwrap();
        if !path.parent().is_some_and(|parent| tree.is_dir(parent)) {
            return Err(not_found(path));
        }
        let start = match tree.nodes.get_mut(path) {
            Some(Node::File { data, .. }) if offset > 0 => {
                data.resize(offset as usize, 0);
                offset
            }
            _ => {
                tree.nodes.insert(path.to_path_buf(), Node::File { data: Vec::new(), modified: None });
                0
            }
        };
        Ok(Writer { stream: Box::new(FakeWriter { tree: self.tree.clone(), path: path.to_path_buf() }), offset: start })
    }

    fn resume_backoff(&self) -> u64 {
        self.resume_backoff
    }
}
