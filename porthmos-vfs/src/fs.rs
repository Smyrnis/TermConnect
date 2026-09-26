use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{DirItem, Entry, Metadata, ProtocolError, SearchQuery, SearchSender, search::walk_search};

pub type Reader = Box<dyn AsyncRead + Send + Unpin>;

pub struct Writer {
    pub stream: Box<dyn AsyncWrite + Send + Unpin>,
    pub offset: u64,
}

#[async_trait]
pub trait FileSystem: Send + Sync {
    async fn list(&self, dir: &Path) -> Result<Vec<Entry>, ProtocolError>;
    async fn read_dir(&self, dir: &Path) -> Result<Vec<DirItem>, ProtocolError>;
    async fn stat(&self, path: &Path) -> Result<Metadata, ProtocolError>;
    async fn create_dir(&self, path: &Path) -> Result<(), ProtocolError>;
    async fn rename(&self, from: &Path, to: &Path) -> Result<(), ProtocolError>;
    async fn remove_file(&self, path: &Path) -> Result<(), ProtocolError>;
    async fn delete(&self, path: &Path) -> Result<(), ProtocolError>;
    async fn home(&self) -> Result<PathBuf, ProtocolError>;
    async fn open_read(&self, path: &Path, offset: u64) -> Result<Reader, ProtocolError>;
    async fn open_write(&self, path: &Path, offset: u64) -> Result<Writer, ProtocolError>;
    async fn open_write_sized(&self, path: &Path, offset: u64, _size: u64) -> Result<Writer, ProtocolError> {
        self.open_write(path, offset).await
    }
    fn resume_backoff(&self) -> u64 {
        0
    }
    async fn search(&self, query: SearchQuery, tx: SearchSender, cancel: Arc<AtomicBool>) {
        walk_search(self, query, tx, cancel).await
    }
}
