use std::fs;
use std::path::Path;

use anyhow::Result;
use std::os::unix::fs::PermissionsExt;

use super::Entry;

pub fn list(path: &Path) -> Result<Vec<Entry>> {
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

pub fn create_directory(path: &Path) -> Result<()> {
    fs::create_dir(path)?;
    Ok(())
}

pub fn rename(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to)?;
    Ok(())
}

pub fn delete(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;

    if metadata.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/filesystem/local_test.rs"]
mod tests;
