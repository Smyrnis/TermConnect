use std::{
    collections::HashMap,
    fmt::Display,
    fs,
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context, Result};

pub mod writer;

pub enum Loaded<T> {
    Missing,
    Ready(T),
    SetAside { warning: String, protected: bool },
}

type PathLock = Arc<Mutex<u64>>;

static WRITE_LOCKS: Mutex<Option<HashMap<PathBuf, PathLock>>> = Mutex::new(None);
static NEXT_VERSION: AtomicU64 = AtomicU64::new(1);

pub fn next_version() -> u64 {
    NEXT_VERSION.fetch_add(1, Ordering::Relaxed)
}

fn write_lock_for(path: &Path) -> PathLock {
    let mut locks = WRITE_LOCKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    locks.get_or_insert_with(HashMap::new).entry(path.to_path_buf()).or_default().clone()
}

fn release_unused_locks() {
    let mut locks = WRITE_LOCKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(table) = locks.as_mut() {
        table.retain(|_, lock| Arc::strong_count(lock) > 1 || lock.try_lock().map_or(true, |version| *version != 0));
    }
}

#[cfg(test)]
fn is_tracked(path: &Path) -> bool {
    WRITE_LOCKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .is_some_and(|table| table.contains_key(path))
}

pub fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    write_locked(path, contents, mode, None)
}

pub fn write_versioned(path: &Path, contents: &[u8], mode: u32, version: u64) -> Result<()> {
    write_locked(path, contents, mode, Some(version))
}

fn write_locked(path: &Path, contents: &[u8], mode: u32, version: Option<u64>) -> Result<()> {
    let lock = write_lock_for(path);
    let outcome = {
        let mut applied = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match version {
            Some(version) if version <= *applied => Ok(()),
            _ => {
                let outcome = write_file(path, contents, mode);
                if let (Ok(()), Some(version)) = (&outcome, version) {
                    *applied = version;
                }
                outcome
            }
        }
    };
    drop(lock);
    release_unused_locks();
    outcome
}

fn write_file(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("couldn't create {}", parent.display()))?;
    }
    let temp_path = with_suffix(path, ".tmp");
    let outcome = write_temp_and_replace(path, &temp_path, contents, mode);
    if outcome.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    outcome
}

fn write_temp_and_replace(path: &Path, temp_path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    let mut handle = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(temp_path)
        .with_context(|| format!("couldn't write {}", temp_path.display()))?;
    handle
        .set_permissions(fs::Permissions::from_mode(mode))
        .with_context(|| format!("couldn't protect {}", temp_path.display()))?;
    handle.write_all(contents).with_context(|| format!("couldn't write {}", temp_path.display()))?;
    drop(handle);
    fs::rename(temp_path, path).with_context(|| format!("couldn't replace {}", path.display()))
}

fn position_of(text: &str, offset: usize) -> Option<(usize, usize)> {
    let before = text.get(..offset)?;
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |last| last.chars().count()) + 1;
    Some((line, column))
}

fn without_quoted_values(message: &str) -> String {
    let mut redacted = String::with_capacity(message.len());
    let mut inside = false;
    for character in message.chars() {
        match (character, inside) {
            ('"', false) => {
                inside = true;
                redacted.push_str("\"\u{2026}\"");
            }
            ('"', true) => inside = false,
            (_, true) => {}
            (other, false) => redacted.push(other),
        }
    }
    redacted
}

pub fn toml_problem(text: &str, error: &toml::de::Error) -> String {
    let message = without_quoted_values(&one_line(error.message()));
    match error.span().and_then(|span| position_of(text, span.start)) {
        Some((line, column)) => format!("line {line}, column {column}: {message}"),
        None => message,
    }
}

pub fn read_or_set_aside<T, E: Display>(
    path: &Path, noun: &str, parse: impl FnOnce(&str) -> Result<T, E>,
) -> Loaded<T> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Loaded::Missing,
        Err(err) => return set_aside(path, &format!("Couldn't read {noun} ({err})")),
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => return set_aside(path, &format!("The {noun} file was unreadable (it is not valid UTF-8)")),
    };
    match parse(&text) {
        Ok(value) => Loaded::Ready(value),
        Err(err) => set_aside(path, &format!("The {noun} file was unreadable ({})", one_line(&err.to_string()))),
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn reserve_broken_name(path: &Path) -> io::Result<PathBuf> {
    let candidates = std::iter::once(with_suffix(path, ".broken"))
        .chain((1..1000).map(|index| with_suffix(path, &format!(".broken.{index}"))));
    for candidate in candidates {
        match fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(_) => return Ok(candidate),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::other("too many .broken files already exist"))
}

fn set_aside<T>(path: &Path, problem: &str) -> Loaded<T> {
    let moved = reserve_broken_name(path).and_then(|broken| match fs::rename(path, &broken) {
        Ok(()) => Ok(broken),
        Err(err) => {
            let _ = fs::remove_file(&broken);
            Err(err)
        }
    });
    match moved {
        Ok(broken) => Loaded::SetAside {
            warning: format!(
                "{problem}; it was kept as {} and a new one was started",
                broken.file_name().and_then(|name| name.to_str()).unwrap_or("the .broken file")
            ),
            protected: false,
        },
        Err(err) => Loaded::SetAside {
            warning: format!("{problem} and couldn't be set aside ({err}); changes won't be saved"),
            protected: true,
        },
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests;
