#[cfg(not(unix))]
compile_error!("termconnect only supports Unix-like platforms (Linux/macOS)");

use std::{
    fs, io,
    io::SeekFrom,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use termconnect_vfs::{DirItem, Entry, FileKind, FileSystem, Metadata, ProtocolError, Reader, Writer, async_trait};
use tokio::io::AsyncSeekExt;

pub struct LocalFs {
    home: PathBuf,
}

impl LocalFs {
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }
}

fn metadata_from(metadata: &fs::Metadata) -> Metadata {
    let kind = if metadata.file_type().is_symlink() {
        FileKind::Symlink
    } else if metadata.is_dir() {
        FileKind::Dir
    } else {
        FileKind::File
    };
    Metadata {
        size: metadata.len(),
        modified: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_secs()),
        kind,
        permissions: Some(metadata.permissions().mode()),
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> Result<T, ProtocolError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|join_error| ProtocolError::from(anyhow::Error::from(join_error)))?
        .map_err(Into::into)
}

#[async_trait]
impl FileSystem for LocalFs {
    async fn list(&self, dir: &Path) -> Result<Vec<Entry>, ProtocolError> {
        let dir = dir.to_path_buf();
        blocking(move || list(&dir)).await
    }

    async fn read_dir(&self, dir: &Path) -> Result<Vec<DirItem>, ProtocolError> {
        let dir = dir.to_path_buf();
        blocking(move || {
            let mut items = Vec::new();
            for dir_entry in fs::read_dir(&dir)? {
                let dir_entry = dir_entry?;
                let metadata = fs::symlink_metadata(dir_entry.path())?;
                items.push(DirItem {
                    name: dir_entry.file_name().to_string_lossy().into_owned(),
                    path: dir_entry.path(),
                    metadata: metadata_from(&metadata),
                });
            }
            Ok(items)
        })
        .await
    }

    async fn stat(&self, path: &Path) -> Result<Metadata, ProtocolError> {
        Ok(metadata_from(&tokio::fs::metadata(path).await?))
    }

    async fn create_dir(&self, path: &Path) -> Result<(), ProtocolError> {
        Ok(tokio::fs::create_dir(path).await?)
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<(), ProtocolError> {
        Ok(tokio::fs::rename(from, to).await?)
    }

    async fn remove_file(&self, path: &Path) -> Result<(), ProtocolError> {
        Ok(tokio::fs::remove_file(path).await?)
    }

    async fn delete(&self, path: &Path) -> Result<(), ProtocolError> {
        let path = path.to_path_buf();
        blocking(move || delete(&path)).await
    }

    async fn home(&self) -> Result<PathBuf, ProtocolError> {
        Ok(self.home.clone())
    }

    async fn open_read(&self, path: &Path, offset: u64) -> Result<Reader, ProtocolError> {
        let mut file = tokio::fs::File::open(path).await?;
        file.seek(SeekFrom::Start(offset)).await?;
        Ok(Box::new(file))
    }

    async fn open_write(&self, path: &Path, offset: u64) -> Result<Writer, ProtocolError> {
        let mut file = tokio::fs::OpenOptions::new().create(true).write(true).truncate(false).open(path).await?;
        file.set_len(offset).await?;
        file.seek(SeekFrom::Start(offset)).await?;
        Ok(Writer { stream: Box::new(file), offset })
    }
}

pub fn list(path: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();

    for dir_entry in fs::read_dir(path)? {
        let dir_entry = dir_entry?;
        let metadata = dir_entry.metadata()?;
        let name = dir_entry.file_name().to_string_lossy().into_owned();

        entries.push(Entry {
            name,
            path: dir_entry.path(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            permissions: Some(metadata.permissions().mode()),
        });
    }

    Ok(entries)
}

pub fn create_directory(path: &Path) -> io::Result<()> {
    fs::create_dir(path)?;
    Ok(())
}

pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)?;
    Ok(())
}

pub fn delete(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;

    if metadata.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests;
