use std::fs;
use std::path::Path;

use anyhow::Result;

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
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

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
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn list_returns_directories_before_files_alphabetically() {
        let dir = tempfile::tempdir().unwrap();
        File::create(dir.path().join("b_file.txt")).unwrap();
        File::create(dir.path().join("a_file.txt")).unwrap();
        fs::create_dir(dir.path().join("z_dir")).unwrap();

        let entries = list(dir.path()).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

        assert_eq!(names, vec!["z_dir", "a_file.txt", "b_file.txt"]);
        assert!(entries[0].is_dir);
        assert!(!entries[1].is_dir);
    }

    #[test]
    fn list_reports_file_size() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("data.txt"), b"hello").unwrap();

        let entries = list(dir.path()).unwrap();

        assert_eq!(entries[0].size, 5);
    }

    #[test]
    fn create_directory_creates_a_new_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("new_dir");

        create_directory(&target).unwrap();

        assert!(target.is_dir());
    }

    #[test]
    fn rename_moves_a_file_to_a_new_name() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("old.txt");
        let renamed = dir.path().join("new.txt");
        fs::write(&original, b"content").unwrap();

        rename(&original, &renamed).unwrap();

        assert!(!original.exists());
        assert!(renamed.exists());
    }

    #[test]
    fn delete_removes_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("doomed.txt");
        fs::write(&file, b"content").unwrap();

        delete(&file).unwrap();

        assert!(!file.exists());
    }

    #[test]
    fn delete_removes_a_directory_and_its_contents() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("doomed_dir");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("inner.txt"), b"content").unwrap();

        delete(&target).unwrap();

        assert!(!target.exists());
    }
}
