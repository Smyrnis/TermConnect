use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{DirItem, Entry, ErrorKind, Metadata, ProtocolError, SearchQuery, SearchSender, search::walk_search};

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
    fn transfer_limit(&self) -> Option<usize> {
        None
    }
    fn resume_backoff(&self) -> u64 {
        0
    }
    async fn set_modified(&self, _path: &Path, _seconds: u64) -> Result<(), ProtocolError> {
        Err(ProtocolError::new(ErrorKind::Unsupported, anyhow::anyhow!("this connection can't set modification times")))
    }

    fn can_set_modified(&self) -> bool {
        false
    }

    fn time_resolution(&self) -> u64 {
        2
    }
    async fn search(&self, query: SearchQuery, tx: SearchSender, cancel: Arc<AtomicBool>) {
        walk_search(self, query, tx, cancel).await
    }
}
