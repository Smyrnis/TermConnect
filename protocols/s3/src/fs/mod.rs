use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use anyhow::anyhow;
use base64::{Engine, engine::general_purpose::STANDARD};
use bytes::Bytes;
use md5::{Digest, Md5};
use porthmos_vfs::{
    DirItem, Entry, ErrorKind, FileKind, FileSystem, Metadata, PART_SUFFIX, ProtocolError, Reader, Writer, async_trait,
};
use reqwest::{
    Method, Response, StatusCode,
    header::{CONTENT_LENGTH, ETAG},
};
use tokio::io::AsyncReadExt;

use crate::{
    client::{Call, S3Client},
    clock::adjust,
    errors::{Failure, to_protocol},
    locate::{Location, encode_key, locate, segments},
    staging::{Progress, Start, completed, contiguous, decide, newest, staged_key},
    upload::{Finish, MultipartWriter, SendPart},
    xml::{self, Cursor, Part, Upload},
};

const DELETE_BATCH: usize = 1_000;
const COPY_LIMIT: u64 = 5 * 1024 * 1024 * 1024;
const COPY_PART: u64 = 512 * 1024 * 1024;

#[derive(Clone)]
struct Started {
    bucket: String,
    key: String,
    id: String,
    parts: BTreeMap<u32, (String, u64)>,
}

impl Started {
    fn upload(&self) -> Upload {
        Upload { key: self.key.clone(), id: self.id.clone(), initiated: None }
    }

    fn parts(&self) -> Vec<Part> {
        self.parts
            .iter()
            .map(|(number, (etag, size))| Part { number: *number, size: *size, etag: etag.clone(), modified: None })
            .collect()
    }
}

type StartedUploads = Arc<Mutex<Vec<Started>>>;

#[derive(Clone)]
pub(crate) struct S3Fs {
    client: Arc<S3Client>,
    bucket: Option<String>,
    started: StartedUploads,
}

fn other(failure: Failure) -> ProtocolError {
    failure.into_error(ErrorKind::Other)
}

fn buckets_refused() -> ProtocolError {
    ProtocolError::new(ErrorKind::PermissionDenied, anyhow!("buckets can't be created or removed here"))
}

fn already_exists(path: &Path) -> ProtocolError {
    ProtocolError::new(ErrorKind::Other, anyhow!("{} already exists", path.display()))
}

fn folder() -> Metadata {
    Metadata { size: 0, modified: None, kind: FileKind::Dir, permissions: None }
}

fn file(size: u64, modified: Option<u64>) -> Metadata {
    Metadata { size, modified, kind: FileKind::File, permissions: None }
}

pub(crate) fn copy_ranges(size: u64, part: u64) -> Vec<(u64, u64)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < size {
        let end = (start + part).min(size) - 1;
        ranges.push((start, end));
        start = end + 1;
    }
    ranges
}

fn panel_path(path: &Path) -> PathBuf {
    let mut normal = PathBuf::from("/");
    normal.extend(segments(path));
    normal
}

async fn checked(client: &S3Client, response: Response, path: &Path) -> Result<Response, ProtocolError> {
    if response.status().is_success() { Ok(response) } else { Err(to_protocol(&client.error(response).await, path)) }
}

fn object(location: &Location) -> Result<(&str, &str), ProtocolError> {
    match location {
        Location::Object { bucket, key } => Ok((bucket, key)),
        Location::Root | Location::Bucket(_) => Err(buckets_refused()),
    }
}

impl S3Fs {
    pub(crate) fn new(client: Arc<S3Client>, bucket: Option<String>) -> Self {
        Self { client, bucket, started: Arc::default() }
    }

    fn started(&self) -> std::sync::MutexGuard<'_, Vec<Started>> {
        self.started.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn remember(&self, bucket: &str, key: &str, id: &str) {
        let mut started = self.started();
        started.retain(|upload| upload.id != id);
        started.push(Started {
            bucket: bucket.to_string(),
            key: key.to_string(),
            id: id.to_string(),
            parts: BTreeMap::new(),
        });
    }

    fn forget(&self, id: &str) {
        self.started().retain(|upload| upload.id != id);
    }

    fn started_upload(&self, bucket: &str, key: &str) -> Option<Started> {
        self.started().iter().rev().find(|upload| upload.bucket == bucket && upload.key == key).cloned()
    }

    async fn known_parts(&self, bucket: &str, key: &str, id: &str, path: &Path) -> Result<Vec<Part>, ProtocolError> {
        match self.parts(bucket, key, id, path).await {
            Err(err) if err.kind() == ErrorKind::PermissionDenied => {
                Ok(self.started().iter().find(|upload| upload.id == id).map(Started::parts).unwrap_or_default())
            }
            result => result,
        }
    }

    fn locate(&self, path: &Path) -> Location {
        locate(path, self.bucket.as_deref())
    }

    async fn send(&self, call: Call<'_>, path: &Path) -> Result<Response, ProtocolError> {
        let response = self.client.send(call).await.map_err(other)?;
        checked(&self.client, response, path).await
    }

    async fn listing(
        &self, bucket: &str, prefix: &str, folders: bool, path: &Path,
    ) -> Result<xml::Listing, ProtocolError> {
        let mut all = xml::Listing::default();
        let mut cursor: Option<Cursor> = None;
        loop {
            let mut call = Call::new(Method::GET, Some(bucket), "").query("list-type", "2").query("prefix", prefix);
            if folders {
                call = call.query("delimiter", "/");
            }
            call = match &cursor {
                Some(Cursor::Token(token)) => call.query("continuation-token", token),
                Some(Cursor::StartAfter(key)) => call.query("start-after", key),
                None => call,
            };
            let response = self.send(call, path).await?;
            let page = xml::listing(&self.client.text(response).await?)?;
            let next = page.cursor(cursor.as_ref())?;
            all.objects.extend(page.objects);
            for prefix in page.prefixes {
                if !all.prefixes.contains(&prefix) {
                    all.prefixes.push(prefix);
                }
            }
            match next {
                Some(next) => cursor = Some(next),
                None => return Ok(all),
            }
        }
    }

    async fn uploads(
        &self, bucket: &str, prefix: &str, folders: bool, path: &Path,
    ) -> Result<Vec<Upload>, ProtocolError> {
        let mut all = Vec::new();
        let mut markers: Option<(String, String)> = None;
        loop {
            let mut call = Call::new(Method::GET, Some(bucket), "").query("uploads", "").query("prefix", prefix);
            if folders {
                call = call.query("delimiter", "/");
            }
            if let Some((key, id)) = &markers {
                call = call.query("key-marker", key).query("upload-id-marker", id);
            }
            let response = self.client.send(call).await.map_err(other)?;
            if matches!(response.status(), StatusCode::FORBIDDEN | StatusCode::NOT_IMPLEMENTED) {
                return Ok(all);
            }
            let response = checked(&self.client, response, path).await?;
            let page = xml::uploads(&self.client.text(response).await?)?;
            all.extend(page.uploads);
            match page.next {
                Some(next) if next != markers.clone().unwrap_or_default() => markers = Some(next),
                _ => return Ok(all),
            }
        }
    }

    async fn parts(&self, bucket: &str, key: &str, id: &str, path: &Path) -> Result<Vec<Part>, ProtocolError> {
        let mut all = Vec::new();
        let mut marker: Option<u32> = None;
        loop {
            let mut call = Call::new(Method::GET, Some(bucket), key).query("uploadId", id);
            if let Some(marker) = marker {
                call = call.query("part-number-marker", &marker.to_string());
            }
            let response = self.send(call, path).await?;
            let page = xml::parts(&self.client.text(response).await?)?;
            all.extend(page.parts);
            match page.next {
                Some(next) if Some(next) != marker => marker = Some(next),
                _ => return Ok(all),
            }
        }
    }

    async fn pending(
        &self, bucket: &str, key: &str, path: &Path,
    ) -> Result<Option<(Upload, Progress, Vec<Part>)>, ProtocolError> {
        let uploads = self.uploads(bucket, key, false, path).await?;
        let upload = match newest(&uploads, key).cloned() {
            Some(upload) => upload,
            None => match self.started_upload(bucket, key) {
                Some(started) => started.upload(),
                None => return Ok(None),
            },
        };
        let parts = self.known_parts(bucket, key, &upload.id, path).await?;
        let progress = contiguous(&parts);
        Ok(Some((upload, progress, parts)))
    }

    async fn abort(&self, bucket: &str, upload: &Upload, path: &Path) -> Result<(), ProtocolError> {
        self.send(Call::new(Method::DELETE, Some(bucket), &upload.key).query("uploadId", &upload.id), path).await?;
        self.forget(&upload.id);
        Ok(())
    }

    async fn abort_all(&self, bucket: &str, key: &str, path: &Path) -> Result<(), ProtocolError> {
        let mut uploads: Vec<Upload> =
            self.uploads(bucket, key, false, path).await?.into_iter().filter(|upload| upload.key == key).collect();
        let started: Vec<Upload> = self
            .started()
            .iter()
            .filter(|upload| upload.bucket == bucket && upload.key == key)
            .map(Started::upload)
            .collect();
        for upload in started {
            if !uploads.iter().any(|known| known.id == upload.id) {
                uploads.push(upload);
            }
        }
        for upload in &uploads {
            self.abort(bucket, upload, path).await?;
        }
        Ok(())
    }

    async fn create_upload(&self, bucket: &str, key: &str, path: &Path) -> Result<String, ProtocolError> {
        let response = self.send(Call::new(Method::POST, Some(bucket), key).query("uploads", ""), path).await?;
        let id = xml::upload_id(&self.client.text(response).await?)?;
        self.remember(bucket, key, &id);
        Ok(id)
    }

    async fn complete(&self, bucket: &str, key: &str, id: &str, path: &Path) -> Result<(), ProtocolError> {
        let parts = completed(&self.known_parts(bucket, key, id, path).await?);
        self.complete_with(bucket, key, id, &parts, path).await
    }

    async fn complete_with(
        &self, bucket: &str, key: &str, id: &str, parts: &[(u32, String)], path: &Path,
    ) -> Result<(), ProtocolError> {
        let call = Call::new(Method::POST, Some(bucket), key)
            .query("uploadId", id)
            .header("content-type", "application/xml")
            .body(xml::complete_body(parts));
        let response = self.send(call, path).await?;
        match xml::completion_error(&self.client.text(response).await?) {
            Some(error) => Err(to_protocol(&error, path)),
            None => {
                self.forget(id);
                Ok(())
            }
        }
    }

    fn part_sender(&self, bucket: &str, key: &str, id: &str, path: &Path) -> SendPart {
        let client = self.client.clone();
        let started = self.started.clone();
        let (bucket, key, id, path) = (bucket.to_string(), key.to_string(), id.to_string(), path.to_path_buf());
        Arc::new(move |number, data: Bytes| {
            let (client, started) = (client.clone(), started.clone());
            let (bucket, key, id, path) = (bucket.clone(), key.clone(), id.clone(), path.clone());
            Box::pin(async move {
                let size = data.len() as u64;
                let call = Call::new(Method::PUT, Some(&bucket), &key)
                    .query("partNumber", &number.to_string())
                    .query("uploadId", &id)
                    .body(data);
                let response = client.send(call).await.map_err(|failure| io::Error::other(other(failure)))?;
                let response = checked(&client, response, &path).await.map_err(io::Error::other)?;
                let etag =
                    response.headers().get(ETAG).and_then(|value| value.to_str().ok()).unwrap_or_default().to_string();
                let mut started = started.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(upload) = started.iter_mut().find(|upload| upload.id == id) {
                    upload.parts.insert(number, (etag, size));
                }
                Ok(())
            })
        })
    }

    fn completion(&self, bucket: &str, key: &str, id: &str, path: &Path) -> Finish {
        let fs = self.clone();
        let (bucket, key, id, path) = (bucket.to_string(), key.to_string(), id.to_string(), path.to_path_buf());
        Box::new(move || {
            Box::pin(async move { fs.complete(&bucket, &key, &id, &path).await.map_err(io::Error::other) })
        })
    }

    async fn delete_keys(&self, bucket: &str, keys: &[String], path: &Path) -> Result<(), ProtocolError> {
        for batch in keys.chunks(DELETE_BATCH) {
            let body = xml::delete_body(batch);
            let digest = STANDARD.encode(Md5::digest(body.as_bytes()));
            let call = Call::new(Method::POST, Some(bucket), "")
                .query("delete", "")
                .header("content-md5", &digest)
                .header("content-type", "application/xml")
                .body(body);
            let response = self.client.send(call).await.map_err(other)?;
            if response.status() == StatusCode::NOT_IMPLEMENTED {
                for key in batch {
                    self.send(Call::new(Method::DELETE, Some(bucket), key), path).await?;
                }
                continue;
            }
            let response = checked(&self.client, response, path).await?;
            if let Some(failure) = xml::delete_failures(&self.client.text(response).await?)?.into_iter().next() {
                return Err(ProtocolError::new(
                    ErrorKind::Other,
                    anyhow!("{}: could not delete {} ({})", path.display(), failure.key, failure.code),
                ));
            }
        }
        Ok(())
    }

    async fn delete_folder(&self, bucket: &str, prefix: &str, path: &Path) -> Result<(), ProtocolError> {
        let keys: Vec<String> =
            self.listing(bucket, prefix, false, path).await?.objects.into_iter().map(|object| object.key).collect();
        self.delete_keys(bucket, &keys, path).await?;
        for upload in self.uploads(bucket, prefix, false, path).await? {
            self.abort(bucket, &upload, path).await?;
        }
        Ok(())
    }

    async fn head(&self, bucket: &str, key: &str, path: &Path) -> Result<Option<Metadata>, ProtocolError> {
        let response = self.client.send(Call::new(Method::HEAD, Some(bucket), key)).await.map_err(other)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = checked(&self.client, response, path).await?;
        let header = |name| response.headers().get(name).and_then(|value| value.to_str().ok()).map(str::to_string);
        let size = header(CONTENT_LENGTH).and_then(|value| value.parse().ok()).unwrap_or(0);
        let modified = header(reqwest::header::LAST_MODIFIED)
            .and_then(|value| httpdate::parse_http_date(&value).ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_secs());
        Ok(Some(file(size, modified)))
    }

    async fn is_folder(&self, bucket: &str, key: &str, path: &Path) -> Result<bool, ProtocolError> {
        let call = Call::new(Method::GET, Some(bucket), "")
            .query("list-type", "2")
            .query("prefix", &format!("{key}/"))
            .query("max-keys", "1");
        let response = self.send(call, path).await?;
        let page = xml::listing(&self.client.text(response).await?)?;
        Ok(!page.objects.is_empty() || !page.prefixes.is_empty())
    }

    async fn find(&self, path: &Path) -> Result<Option<Metadata>, ProtocolError> {
        match self.locate(path) {
            Location::Root | Location::Bucket(_) => Ok(Some(folder())),
            Location::Object { bucket, key } => {
                if let Some(staged) = staged_key(&key)
                    && let Some((upload, progress, _)) = self.pending(&bucket, staged, path).await?
                {
                    let modified = adjust(progress.modified.or(upload.initiated), self.client.skew());
                    return Ok(Some(file(progress.size, modified)));
                }
                if let Some(metadata) = self.head(&bucket, &key, path).await? {
                    return Ok(Some(metadata));
                }
                Ok(self.is_folder(&bucket, &key, path).await?.then(folder))
            }
        }
    }

    async fn copy(&self, bucket: &str, from: &str, to: &str, size: u64, path: &Path) -> Result<(), ProtocolError> {
        let source = format!("/{}/{}", encode_key(bucket), encode_key(from));
        if size <= COPY_LIMIT {
            let response =
                self.send(Call::new(Method::PUT, Some(bucket), to).header("x-amz-copy-source", &source), path).await?;
            if let Some(error) = xml::completion_error(&self.client.text(response).await?) {
                return Err(to_protocol(&error, path));
            }
        } else {
            let id = self.create_upload(bucket, to, path).await?;
            if let Err(err) = self.copy_parts(bucket, &source, to, &id, size, path).await {
                let upload = Upload { key: to.to_string(), id, initiated: None };
                let _ = self.abort(bucket, &upload, path).await;
                return Err(err);
            }
        }
        match self.head(bucket, to, path).await? {
            Some(copied) if copied.size == size => Ok(()),
            _ => {
                let _ = self.send(Call::new(Method::DELETE, Some(bucket), to), path).await;
                Err(ProtocolError::new(ErrorKind::Other, anyhow!("the copy of {} is incomplete", path.display())))
            }
        }
    }

    async fn copy_parts(
        &self, bucket: &str, source: &str, to: &str, id: &str, size: u64, path: &Path,
    ) -> Result<(), ProtocolError> {
        let mut parts = Vec::new();
        for (index, (start, end)) in copy_ranges(size, COPY_PART).into_iter().enumerate() {
            let number = index as u32 + 1;
            let call = Call::new(Method::PUT, Some(bucket), to)
                .query("partNumber", &number.to_string())
                .query("uploadId", id)
                .header("x-amz-copy-source", source)
                .header("x-amz-copy-source-range", &format!("bytes={start}-{end}"));
            let reply = self.client.text(self.send(call, path).await?).await?;
            if let Some(error) = xml::completion_error(&reply) {
                return Err(to_protocol(&error, path));
            }
            let etag = xml::copy_part_etag(&reply).ok_or_else(|| {
                ProtocolError::new(
                    ErrorKind::Other,
                    anyhow!("the server did not confirm a copied part of {}", path.display()),
                )
            })?;
            parts.push((number, etag));
        }
        self.complete_with(bucket, to, id, &parts, path).await
    }

    async fn move_folder(&self, bucket: &str, from: &str, to: &str, path: &Path) -> Result<(), ProtocolError> {
        let objects = self.listing(bucket, &format!("{from}/"), false, path).await?.objects;
        for object in &objects {
            let target = format!("{to}{}", &object.key[from.len()..]);
            if let Err(err) = self.copy(bucket, &object.key, &target, object.size, path).await {
                return Err(ProtocolError::new(
                    ErrorKind::Other,
                    anyhow!("{}: could not move {} ({err})", path.display(), object.key),
                ));
            }
        }
        let keys: Vec<String> = objects.into_iter().map(|object| object.key).collect();
        self.delete_keys(bucket, &keys, path).await?;
        for upload in self.uploads(bucket, &format!("{from}/"), false, path).await? {
            self.abort(bucket, &upload, path).await?;
        }
        Ok(())
    }

    async fn items(&self, dir: &Path) -> Result<Vec<DirItem>, ProtocolError> {
        let base = panel_path(dir);
        let (bucket, prefix) = match self.locate(dir) {
            Location::Root => {
                let response = self.send(Call::new(Method::GET, None, ""), dir).await?;
                return Ok(xml::buckets(&self.client.text(response).await?)?
                    .into_iter()
                    .map(|bucket| DirItem {
                        path: base.join(&bucket.name),
                        metadata: Metadata { modified: bucket.created, ..folder() },
                        name: bucket.name,
                    })
                    .collect());
            }
            location => (location.bucket().unwrap_or_default().to_string(), location.prefix()),
        };
        let listing = self.listing(&bucket, &prefix, true, dir).await?;
        let mut items: Vec<DirItem> = Vec::new();
        for folder_prefix in &listing.prefixes {
            let name = folder_prefix[prefix.len().min(folder_prefix.len())..].trim_end_matches('/').to_string();
            if !name.is_empty() && !name.contains('/') {
                items.push(DirItem { path: base.join(&name), metadata: folder(), name });
            }
        }
        for object in &listing.objects {
            let name = object.key[prefix.len().min(object.key.len())..].to_string();
            if !name.is_empty() && !name.contains('/') {
                items.push(DirItem { path: base.join(&name), metadata: file(object.size, object.modified), name });
            }
        }
        let uploads = self.uploads(&bucket, &prefix, true, dir).await?;
        let mut seen: Vec<&str> = Vec::new();
        for upload in &uploads {
            if seen.contains(&upload.key.as_str()) {
                continue;
            }
            seen.push(&upload.key);
            let Some(newest) = newest(&uploads, &upload.key) else {
                continue;
            };
            let name = format!("{}{PART_SUFFIX}", &newest.key[prefix.len().min(newest.key.len())..]);
            if name.contains('/') || items.iter().any(|item| item.name == name) {
                continue;
            }
            let progress = contiguous(&self.known_parts(&bucket, &newest.key, &newest.id, dir).await?);
            items.push(DirItem {
                path: base.join(&name),
                metadata: file(progress.size, adjust(progress.modified.or(newest.initiated), self.client.skew())),
                name,
            });
        }
        Ok(items)
    }
}

#[async_trait]
impl FileSystem for S3Fs {
    async fn list(&self, dir: &Path) -> Result<Vec<Entry>, ProtocolError> {
        Ok(self
            .items(dir)
            .await?
            .into_iter()
            .map(|item| Entry {
                is_dir: item.metadata.is_dir(),
                size: item.metadata.size,
                permissions: None,
                name: item.name,
                path: item.path,
            })
            .collect())
    }

    async fn read_dir(&self, dir: &Path) -> Result<Vec<DirItem>, ProtocolError> {
        self.items(dir).await
    }

    async fn stat(&self, path: &Path) -> Result<Metadata, ProtocolError> {
        self.find(path)
            .await?
            .ok_or_else(|| ProtocolError::new(ErrorKind::NotFound, anyhow!("{} not found", path.display())))
    }

    async fn create_dir(&self, path: &Path) -> Result<(), ProtocolError> {
        let location = self.locate(path);
        let (bucket, key) = object(&location)?;
        self.send(Call::new(Method::PUT, Some(bucket), &format!("{key}/")), path).await?;
        Ok(())
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<(), ProtocolError> {
        let (source, target) = (self.locate(from), self.locate(to));
        let ((bucket, from_key), (to_bucket, to_key)) = (object(&source)?, object(&target)?);
        if bucket != to_bucket {
            return Err(ProtocolError::new(ErrorKind::Other, anyhow!("moving between buckets is not supported")));
        }
        if from_key == to_key {
            return Ok(());
        }
        if let Some(staged) = staged_key(from_key).filter(|staged| *staged == to_key)
            && let Some((upload, _, _)) = self.pending(bucket, staged, from).await?
        {
            return self.complete(bucket, staged, &upload.id, from).await;
        }
        let source_metadata = self.stat(from).await?;
        let existing = self.find(to).await?;
        if source_metadata.is_dir() {
            if existing.is_some() {
                return Err(already_exists(to));
            }
            return self.move_folder(bucket, from_key, to_key, from).await;
        }
        if existing.is_some_and(|metadata| metadata.is_dir()) {
            return Err(already_exists(to));
        }
        self.copy(bucket, from_key, to_key, source_metadata.size, from).await?;
        self.send(Call::new(Method::DELETE, Some(bucket), from_key), from).await?;
        Ok(())
    }

    async fn remove_file(&self, path: &Path) -> Result<(), ProtocolError> {
        let location = self.locate(path);
        let (bucket, key) = object(&location)?;
        if let Some(staged) = staged_key(key)
            && self.pending(bucket, staged, path).await?.is_some()
        {
            return self.abort_all(bucket, staged, path).await;
        }
        self.send(Call::new(Method::DELETE, Some(bucket), key), path).await?;
        Ok(())
    }

    async fn delete(&self, path: &Path) -> Result<(), ProtocolError> {
        let location = self.locate(path);
        let (bucket, key) = object(&location)?;
        if self.stat(path).await?.is_dir() {
            return self.delete_folder(bucket, &format!("{key}/"), path).await;
        }
        self.remove_file(path).await
    }

    async fn home(&self) -> Result<PathBuf, ProtocolError> {
        Ok(PathBuf::from("/"))
    }

    async fn open_read(&self, path: &Path, offset: u64) -> Result<Reader, ProtocolError> {
        let location = self.locate(path);
        let (bucket, key) = object(&location)?;
        let mut call = Call::new(Method::GET, Some(bucket), key);
        if offset > 0 {
            call = call.header("range", &format!("bytes={offset}-"));
        }
        let response = self.client.send(call).await.map_err(other)?;
        let status = response.status();
        if status == StatusCode::RANGE_NOT_SATISFIABLE && offset > 0 {
            return Ok(Box::new(tokio::io::empty()));
        }
        let response = checked(&self.client, response, path).await?;
        let mut reader = self.client.reader(response);
        if status != StatusCode::PARTIAL_CONTENT && offset > 0 {
            let skipped = tokio::io::copy(&mut (&mut reader).take(offset), &mut tokio::io::sink()).await?;
            if skipped < offset {
                return Err(ProtocolError::new(
                    ErrorKind::Other,
                    anyhow!("the server sent fewer bytes than the resume offset"),
                ));
            }
        }
        Ok(Box::new(reader))
    }

    async fn open_write(&self, path: &Path, offset: u64) -> Result<Writer, ProtocolError> {
        let location = self.locate(path);
        let (bucket, key) = object(&location)?;
        let too_large = format!("{} is too large for S3 multipart upload", path.display());
        let Some(staged) = staged_key(key) else {
            let id = self.create_upload(bucket, key, path).await?;
            let finish = self.completion(bucket, key, &id, path);
            let writer = MultipartWriter::new(self.part_sender(bucket, key, &id, path), 1, Some(finish), too_large);
            return Ok(Writer { stream: Box::new(writer), offset: 0 });
        };
        let found = self.pending(bucket, staged, path).await?.map(|(upload, progress, _)| (upload.id, progress));
        match decide(offset, found) {
            Start::Continue { id, next_part } => {
                let writer =
                    MultipartWriter::new(self.part_sender(bucket, staged, &id, path), next_part, None, too_large);
                Ok(Writer { stream: Box::new(writer), offset })
            }
            Start::Restart => {
                self.abort_all(bucket, staged, path).await?;
                let id = self.create_upload(bucket, staged, path).await?;
                let writer = MultipartWriter::new(self.part_sender(bucket, staged, &id, path), 1, None, too_large);
                Ok(Writer { stream: Box::new(writer), offset: 0 })
            }
        }
    }
}

#[cfg(test)]
mod tests;
