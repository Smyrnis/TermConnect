use std::{
    collections::hash_map::DefaultHasher,
    fs::File,
    hash::Hasher,
    io::{self, Read},
    path::Path,
};

use chrono::{DateTime, Utc};
use porthmos_vfs::Environment;

pub const MAX_EDIT_BYTES: u64 = 64 * 1024 * 1024;

const FALLBACK_EDITOR: &str = "vi";
const HASH_CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorExit {
    Success,
    Status(i32),
    LaunchFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditChoice {
    Upload,
    KeepCopy,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditQuestionKind {
    Upload,
    Conflict,
}

pub fn split_command(text: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        match (quote, character) {
            (Some(open), close) if close == open => quote = None,
            (Some('"'), '\\') => match characters.next() {
                Some(next @ ('"' | '\\')) => current.push(next),
                Some(next) => {
                    current.push('\\');
                    current.push(next);
                }
                None => return Err("the command ends with a backslash".to_string()),
            },
            (Some(_), other) => current.push(other),
            (None, '\'' | '"') => {
                quote = Some(character);
                in_word = true;
            }
            (None, '\\') => match characters.next() {
                Some(next) => {
                    current.push(next);
                    in_word = true;
                }
                None => return Err("the command ends with a backslash".to_string()),
            },
            (None, space) if space.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            (None, other) => {
                current.push(other);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return Err("the command has an unterminated quote".to_string());
    }
    if in_word {
        words.push(current);
    }
    Ok(words)
}

pub fn resolve_editor(setting: Option<&str>, env: &Environment) -> Result<EditorCommand, String> {
    let usable = |text: &&str| !text.trim().is_empty();
    let text = setting.filter(usable).or(env.editor.as_deref().filter(usable)).unwrap_or(FALLBACK_EDITOR);
    let mut words = split_command(text)?;
    if words.is_empty() || words[0].is_empty() {
        return Err("the editor command is empty".to_string());
    }
    let program = words.remove(0);
    Ok(EditorCommand { program, args: words })
}

pub fn printable(text: &str) -> String {
    text.chars().map(|character| if character.is_control() { '?' } else { character }).collect()
}

pub fn conflict_copy_name(name: &str, at: DateTime<Utc>) -> String {
    let stamp = at.format("%Y%m%d-%H%M%S");
    match name.rfind('.') {
        Some(index) if index > 0 => format!("{}.conflict-{stamp}{}", &name[..index], &name[index..]),
        _ => format!("{name}.conflict-{stamp}"),
    }
}

pub fn hash_file(path: &Path) -> io::Result<u64> {
    let mut file = File::open(path)?;
    let mut hasher = DefaultHasher::new();
    let mut buffer = vec![0u8; HASH_CHUNK];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(hasher.finish());
        }
        hasher.write(&buffer[..read]);
    }
}

#[cfg(test)]
mod tests;
