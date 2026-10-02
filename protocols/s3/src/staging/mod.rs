use porthmos_vfs::PART_SUFFIX;

use crate::{
    upload::PART_SIZE,
    xml::{Part, Upload},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Progress {
    pub(crate) last: u32,
    pub(crate) size: u64,
    pub(crate) modified: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Start {
    Continue { id: String, next_part: u32 },
    Restart,
}

pub(crate) fn staged_key(key: &str) -> Option<&str> {
    key.strip_suffix(PART_SUFFIX).filter(|name| !name.is_empty() && !name.ends_with('/'))
}

fn in_order(parts: &[Part]) -> Vec<&Part> {
    let mut sorted: Vec<&Part> = parts.iter().collect();
    sorted.sort_by_key(|part| part.number);
    sorted
        .into_iter()
        .enumerate()
        .take_while(|(index, part)| part.number as usize == index + 1)
        .map(|(_, part)| part)
        .collect()
}

pub(crate) fn contiguous(parts: &[Part]) -> Progress {
    let parts: Vec<&Part> = in_order(parts).into_iter().take_while(|part| part.size == PART_SIZE as u64).collect();
    Progress {
        last: parts.last().map_or(0, |part| part.number),
        size: parts.iter().map(|part| part.size).sum(),
        modified: parts.iter().filter_map(|part| part.modified).max(),
    }
}

pub(crate) fn completed(parts: &[Part]) -> Vec<(u32, String)> {
    in_order(parts).into_iter().map(|part| (part.number, part.etag.clone())).collect()
}

pub(crate) fn newest<'a>(uploads: &'a [Upload], key: &str) -> Option<&'a Upload> {
    uploads.iter().filter(|upload| upload.key == key).max_by_key(|upload| upload.initiated)
}

pub(crate) fn decide(offset: u64, found: Option<(String, Progress)>) -> Start {
    match found {
        Some((id, progress)) if offset > 0 && progress.size == offset => {
            Start::Continue { id, next_part: progress.last + 1 }
        }
        _ => Start::Restart,
    }
}

#[cfg(test)]
mod tests;
