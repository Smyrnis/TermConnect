#![allow(dead_code)]

use std::{
    collections::VecDeque,
    convert::Infallible,
    sync::{Arc, Mutex},
};

use hyper::{
    Request, Response, StatusCode, body::Incoming, header::HeaderValue, server::conn::http1, service::service_fn,
};
use hyper_util::rt::TokioIo;
use porthmos_vfs::{Answer, Prompter, Question, Target, async_trait};
use s3s::{Body, S3, S3Request, S3Response, S3Result, dto::*, service::S3ServiceBuilder};
use tokio::net::TcpListener;

pub const ACCESS_KEY: &str = "AKIDTEST";
pub const SECRET: &str = "test-secret";
pub const BUCKET: &str = "bkt";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quirk {
    None,
    RegionRedirect(&'static str),
    DenyListBuckets,
    NoDeleteObjects,
    PageSize(i32),
    HangGet,
    ClockSkew,
    DenyUploadListing,
    BucketRegion(&'static str, &'static str),
    CopyErrorInSuccess,
    DenyPartListing,
    ShortCopy,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub quirk: Quirk,
    pub buckets: &'static [&'static str],
}

impl Default for Options {
    fn default() -> Self {
        Self { quirk: Quirk::None, buckets: &[BUCKET] }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub bucket: String,
    pub key: String,
    pub id: String,
}

#[derive(Default)]
pub struct Registry {
    pub uploads: Mutex<Vec<Pending>>,
    etags: Mutex<Vec<((String, i32), String)>>,
    sizes: Mutex<Vec<((String, i32), i64)>>,
    markers: Mutex<Vec<(String, String)>>,
}

fn marker_listing(
    markers: &[(String, String)], bucket: &str, output: &mut ListObjectsV2Output, prefix: &str, delimiter: Option<&str>,
) {
    for (_, key) in markers.iter().filter(|(owner, key)| owner == bucket && key.starts_with(prefix)) {
        let rest = &key[prefix.len()..];
        match delimiter.and_then(|slash| rest.find(slash)) {
            Some(position) => {
                let common = format!("{prefix}{}", &rest[..=position]);
                let prefixes = output.common_prefixes.get_or_insert_with(Vec::new);
                if !prefixes.iter().any(|known| known.prefix.as_deref() == Some(common.as_str())) {
                    prefixes.push(CommonPrefix { prefix: Some(common) });
                }
            }
            _ => {
                let contents = output.contents.get_or_insert_with(Vec::new);
                if !contents.iter().any(|object| object.key.as_deref() == Some(key.as_str())) {
                    contents.push(Object { key: Some(key.clone()), size: Some(0), ..Default::default() });
                }
            }
        }
    }
}

impl Wrap {
    fn is_directory(&self, bucket: &str, key: &str) -> bool {
        !key.ends_with('/') && self.root.join(bucket).join(key).is_dir()
    }

    fn forget_marker(&self, bucket: &str, key: &str) {
        self.registry.markers.lock().unwrap().retain(|(owner, known)| !(owner == bucket && known == key));
    }
}

impl Registry {
    pub fn pending(&self) -> Vec<Pending> {
        self.uploads.lock().unwrap().clone()
    }

    fn forget(&self, id: &str) {
        self.uploads.lock().unwrap().retain(|upload| upload.id != id);
    }
}

struct Wrap {
    fs: s3s_fs::FileSystem,
    root: std::path::PathBuf,
    registry: Arc<Registry>,
    quirk: Quirk,
}

#[async_trait::async_trait]
impl S3 for Wrap {
    async fn list_buckets(&self, req: S3Request<ListBucketsInput>) -> S3Result<S3Response<ListBucketsOutput>> {
        self.fs.list_buckets(req).await
    }

    async fn head_bucket(&self, req: S3Request<HeadBucketInput>) -> S3Result<S3Response<HeadBucketOutput>> {
        self.fs.head_bucket(req).await
    }

    async fn list_objects_v2(
        &self, mut req: S3Request<ListObjectsV2Input>,
    ) -> S3Result<S3Response<ListObjectsV2Output>> {
        if let Quirk::PageSize(size) = self.quirk {
            req.input.max_keys = Some(req.input.max_keys.map_or(size, |asked| asked.min(size)));
        }
        let bucket = req.input.bucket.clone();
        let prefix = req.input.prefix.clone().unwrap_or_default();
        let delimiter = req.input.delimiter.clone();
        let mut output = self.fs.list_objects_v2(req).await?;
        if output.output.is_truncated != Some(true) {
            let markers = self.registry.markers.lock().unwrap().clone();
            marker_listing(&markers, &bucket, &mut output.output, &prefix, delimiter.as_deref());
        }
        Ok(output)
    }

    async fn head_object(&self, req: S3Request<HeadObjectInput>) -> S3Result<S3Response<HeadObjectOutput>> {
        if self.is_directory(&req.input.bucket, &req.input.key) {
            return Err(s3s::s3_error!(NoSuchKey));
        }
        self.fs.head_object(req).await
    }

    async fn get_object(&self, req: S3Request<GetObjectInput>) -> S3Result<S3Response<GetObjectOutput>> {
        if self.is_directory(&req.input.bucket, &req.input.key) {
            return Err(s3s::s3_error!(NoSuchKey));
        }
        self.fs.get_object(req).await
    }

    async fn put_object(&self, req: S3Request<PutObjectInput>) -> S3Result<S3Response<PutObjectOutput>> {
        if req.input.key.ends_with('/') {
            let marker = (req.input.bucket.clone(), req.input.key.clone());
            let mut markers = self.registry.markers.lock().unwrap();
            if !markers.contains(&marker) {
                markers.push(marker);
            }
        }
        self.fs.put_object(req).await
    }

    async fn delete_object(&self, req: S3Request<DeleteObjectInput>) -> S3Result<S3Response<DeleteObjectOutput>> {
        if req.input.key.ends_with('/') {
            self.forget_marker(&req.input.bucket, &req.input.key);
            return Ok(S3Response::new(DeleteObjectOutput::default()));
        }
        self.fs.delete_object(req).await
    }

    async fn delete_objects(
        &self, mut req: S3Request<DeleteObjectsInput>,
    ) -> S3Result<S3Response<DeleteObjectsOutput>> {
        let bucket = req.input.bucket.clone();
        req.input.delete.objects.retain(|object| {
            if object.key.ends_with('/') {
                self.forget_marker(&bucket, &object.key);
                false
            } else {
                true
            }
        });
        self.fs.delete_objects(req).await
    }

    async fn copy_object(&self, req: S3Request<CopyObjectInput>) -> S3Result<S3Response<CopyObjectOutput>> {
        if req.input.key.ends_with('/') {
            std::fs::create_dir_all(self.root.join(&req.input.bucket).join(&req.input.key)).unwrap();
            self.registry.markers.lock().unwrap().push((req.input.bucket.clone(), req.input.key.clone()));
            return Ok(S3Response::new(CopyObjectOutput {
                copy_object_result: Some(CopyObjectResult::default()),
                ..Default::default()
            }));
        }
        let target = self.root.join(&req.input.bucket).join(&req.input.key);
        let output = self.fs.copy_object(req).await?;
        if self.quirk == Quirk::ShortCopy {
            let length = std::fs::metadata(&target).map(|metadata| metadata.len()).unwrap_or(0);
            std::fs::OpenOptions::new().write(true).open(&target).unwrap().set_len(length / 2).unwrap();
        }
        Ok(output)
    }

    async fn create_multipart_upload(
        &self, req: S3Request<CreateMultipartUploadInput>,
    ) -> S3Result<S3Response<CreateMultipartUploadOutput>> {
        let (bucket, key) = (req.input.bucket.clone(), req.input.key.clone());
        let output = self.fs.create_multipart_upload(req).await?;
        let id = output.output.upload_id.clone().unwrap_or_default();
        self.registry.uploads.lock().unwrap().push(Pending { bucket, key, id });
        Ok(output)
    }

    async fn upload_part(&self, req: S3Request<UploadPartInput>) -> S3Result<S3Response<UploadPartOutput>> {
        let part = (req.input.upload_id.clone(), req.input.part_number);
        let size = req.input.content_length.unwrap_or_default();
        {
            let mut sizes = self.registry.sizes.lock().unwrap();
            sizes.retain(|(known, _)| *known != part);
            sizes.push((part.clone(), size));
        }
        let output = self.fs.upload_part(req).await?;
        if let Some(etag) = &output.output.e_tag {
            let mut etags = self.registry.etags.lock().unwrap();
            etags.retain(|(known, _)| *known != part);
            etags.push((part, etag.value().to_string()));
        }
        Ok(output)
    }

    async fn upload_part_copy(
        &self, req: S3Request<UploadPartCopyInput>,
    ) -> S3Result<S3Response<UploadPartCopyOutput>> {
        let part = (req.input.upload_id.clone(), req.input.part_number);
        let output = self.fs.upload_part_copy(req).await?;
        if let Some(etag) = output.output.copy_part_result.as_ref().and_then(|result| result.e_tag.as_ref()) {
            let mut etags = self.registry.etags.lock().unwrap();
            etags.retain(|(known, _)| *known != part);
            etags.push((part, etag.value().to_string()));
        }
        Ok(output)
    }

    async fn list_parts(&self, req: S3Request<ListPartsInput>) -> S3Result<S3Response<ListPartsOutput>> {
        let id = req.input.upload_id.clone();
        let mut output = self.fs.list_parts(req).await?;
        let etags = self.registry.etags.lock().unwrap();
        for part in output.output.parts.iter_mut().flatten() {
            let wanted = (id.clone(), part.part_number.unwrap_or_default());
            if let Some((_, etag)) = etags.iter().find(|(known, _)| *known == wanted) {
                part.e_tag = Some(ETag::Strong(etag.clone()));
            }
        }
        Ok(output)
    }

    async fn complete_multipart_upload(
        &self, req: S3Request<CompleteMultipartUploadInput>,
    ) -> S3Result<S3Response<CompleteMultipartUploadOutput>> {
        let id = req.input.upload_id.clone();
        let numbers: Vec<i32> = req
            .input
            .multipart_upload
            .as_ref()
            .and_then(|upload| upload.parts.as_ref())
            .map(|parts| parts.iter().filter_map(|part| part.part_number).collect())
            .unwrap_or_default();
        let last = numbers.iter().copied().max().unwrap_or_default();
        let sizes = self.registry.sizes.lock().unwrap().clone();
        let too_small = numbers.iter().filter(|number| **number != last).any(|number| {
            sizes
                .iter()
                .find(|((known, part), _)| *known == id && part == number)
                .is_none_or(|(_, size)| *size < 5 * 1024 * 1024)
        });
        if too_small {
            return Err(s3s::s3_error!(EntityTooSmall, "a part other than the last is smaller than 5 MiB"));
        }
        let non_final: Vec<i64> = numbers
            .iter()
            .filter(|number| **number != last)
            .filter_map(|number| {
                sizes.iter().find(|((known, part), _)| *known == id && part == number).map(|(_, size)| *size)
            })
            .collect();
        let final_size = sizes.iter().find(|((known, part), _)| *known == id && *part == last).map(|(_, size)| *size);
        let uneven = non_final.windows(2).any(|pair| pair[0] != pair[1])
            || non_final.first().zip(final_size).is_some_and(|(first, last)| last > *first);
        if uneven {
            return Err(s3s::s3_error!(InvalidPart, "All non-trailing parts must have the same length"));
        }
        let output = self.fs.complete_multipart_upload(req).await?;
        self.registry.forget(&id);
        Ok(output)
    }

    async fn abort_multipart_upload(
        &self, req: S3Request<AbortMultipartUploadInput>,
    ) -> S3Result<S3Response<AbortMultipartUploadOutput>> {
        let id = req.input.upload_id.clone();
        let output = self.fs.abort_multipart_upload(req).await?;
        self.registry.forget(&id);
        Ok(output)
    }

    async fn list_multipart_uploads(
        &self, req: S3Request<ListMultipartUploadsInput>,
    ) -> S3Result<S3Response<ListMultipartUploadsOutput>> {
        let prefix = req.input.prefix.clone().unwrap_or_default();
        let delimiter = req.input.delimiter.clone();
        let uploads: Vec<MultipartUpload> = self
            .registry
            .pending()
            .into_iter()
            .filter(|upload| upload.bucket == req.input.bucket && upload.key.starts_with(&prefix))
            .filter(|upload| {
                delimiter.as_ref().is_none_or(|slash| !upload.key[prefix.len()..].contains(slash.as_str()))
            })
            .map(|upload| MultipartUpload { key: Some(upload.key), upload_id: Some(upload.id), ..Default::default() })
            .collect();
        Ok(S3Response::new(ListMultipartUploadsOutput {
            bucket: Some(req.input.bucket.clone()),
            uploads: Some(uploads),
            ..Default::default()
        }))
    }
}

pub struct TlsFiles {
    pub der: Vec<u8>,
    pub key_der: Vec<u8>,
}

pub fn self_signed_tls() -> TlsFiles {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()]).unwrap();
    TlsFiles { der: certified.cert.der().to_vec(), key_der: certified.signing_key.serialize_der() }
}

fn acceptor(tls: &TlsFiles) -> tokio_rustls::TlsAcceptor {
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(tls.der.clone())],
            rustls::pki_types::PrivateKeyDer::try_from(tls.key_der.clone()).unwrap(),
        )
        .unwrap();
    tokio_rustls::TlsAcceptor::from(Arc::new(config))
}

pub struct Server {
    pub port: u16,
    pub root: tempfile::TempDir,
    pub registry: Arc<Registry>,
}

impl Server {
    pub fn bucket_dir(&self) -> std::path::PathBuf {
        self.root.path().join(BUCKET)
    }
}

fn xml_error(status: StatusCode, code: &str, region: Option<&str>) -> Response<Body> {
    let region_element = region.map(|region| format!("<Region>{region}</Region>")).unwrap_or_default();
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>{code}</Code><Message>{code}</Message>{region_element}</Error>"
    );
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    if let Some(region) = region {
        response.headers_mut().insert("x-amz-bucket-region", HeaderValue::from_str(region).unwrap());
    }
    response
}

fn quirk_response(quirk: Quirk, request: &Request<Incoming>) -> Option<Response<Body>> {
    let path = request.uri().path();
    let query = request.uri().query().unwrap_or_default();
    match quirk {
        Quirk::ClockSkew => Some(xml_error(StatusCode::FORBIDDEN, "RequestTimeTooSkewed", None)),
        Quirk::DenyListBuckets if path == "/" && request.method() == "GET" => {
            Some(xml_error(StatusCode::FORBIDDEN, "AccessDenied", None))
        }
        Quirk::NoDeleteObjects
            if request.method() == "POST" && query.split('&').any(|item| item == "delete" || item == "delete=") =>
        {
            Some(xml_error(StatusCode::NOT_IMPLEMENTED, "NotImplemented", None))
        }
        Quirk::DenyUploadListing
            if request.method() == "GET" && query.split('&').any(|item| item == "uploads" || item == "uploads=") =>
        {
            Some(xml_error(StatusCode::FORBIDDEN, "AccessDenied", None))
        }
        Quirk::BucketRegion(bucket, region)
            if path.trim_start_matches('/').split('/').next() == Some(bucket)
                && !request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| value.contains(&format!("/{region}/s3/"))) =>
        {
            Some(xml_error(StatusCode::MOVED_PERMANENTLY, "PermanentRedirect", Some(region)))
        }
        Quirk::DenyPartListing
            if request.method() == "GET" && query.split('&').any(|item| item.starts_with("uploadId=")) =>
        {
            Some(xml_error(StatusCode::FORBIDDEN, "AccessDenied", None))
        }
        Quirk::CopyErrorInSuccess if request.headers().contains_key("x-amz-copy-source") => {
            let mut response = xml_error(StatusCode::OK, "InternalError", None);
            response.headers_mut().remove("x-amz-bucket-region");
            Some(response)
        }
        Quirk::RegionRedirect(region) => {
            let signed_for = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains(&format!("/{region}/s3/")));
            (!signed_for).then(|| xml_error(StatusCode::MOVED_PERMANENTLY, "PermanentRedirect", Some(region)))
        }
        _ => None,
    }
}

pub async fn start(options: Options) -> Server {
    start_with_tls(options, None).await
}

pub async fn start_with_tls(options: Options, tls: Option<&TlsFiles>) -> Server {
    let root = tempfile::tempdir().unwrap();
    for bucket in options.buckets {
        std::fs::create_dir_all(root.path().join(bucket)).unwrap();
    }
    let registry = Arc::new(Registry::default());
    let wrap = Wrap {
        fs: s3s_fs::FileSystem::new(root.path()).unwrap(),
        root: root.path().to_path_buf(),
        registry: registry.clone(),
        quirk: options.quirk,
    };
    let mut builder = S3ServiceBuilder::new(wrap);
    builder.set_auth(s3s::auth::SimpleAuth::from_single(ACCESS_KEY, SECRET));
    let service = builder.build();
    let acceptor = tls.map(acceptor);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let quirk = options.quirk;
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let service = service.clone();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let handler = service_fn(move |request: Request<Incoming>| {
                    let service = service.clone();
                    async move {
                        if quirk == Quirk::HangGet
                            && request.method() == "GET"
                            && request.uri().path().trim_start_matches('/').contains('/')
                        {
                            std::future::pending::<()>().await;
                        }
                        if let Some(response) = quirk_response(quirk, &request) {
                            return Ok::<_, Infallible>(response);
                        }
                        Ok(service
                            .call(request.map(Body::from))
                            .await
                            .unwrap_or_else(|_| xml_error(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", None)))
                    }
                });
                match acceptor {
                    Some(acceptor) => {
                        if let Ok(stream) = acceptor.accept(stream).await {
                            let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), handler).await;
                        }
                    }
                    None => {
                        let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), handler).await;
                    }
                }
            });
        }
    });
    Server { port, root, registry }
}

pub struct Answers {
    pub answers: VecDeque<Option<Answer>>,
    pub asked: Vec<Question>,
}

pub fn answers(answers: Vec<Option<Answer>>) -> Answers {
    Answers { answers: answers.into(), asked: Vec::new() }
}

#[async_trait]
impl Prompter for Answers {
    async fn ask(&mut self, question: Question) -> Option<Answer> {
        self.asked.push(question);
        self.answers.pop_front().flatten()
    }
}

pub fn target(port: u16, bucket: Option<&str>, secret: Option<&str>) -> Target {
    let mut options: std::collections::BTreeMap<String, String> = [
        ("security".to_string(), "http".to_string()),
        ("addressing".to_string(), "path".to_string()),
        ("region".to_string(), "us-east-1".to_string()),
    ]
    .into_iter()
    .collect();
    if let Some(bucket) = bucket {
        options.insert("bucket".to_string(), bucket.to_string());
    }
    Target {
        name: "store".into(),
        host: "127.0.0.1".into(),
        port,
        username: ACCESS_KEY.into(),
        password: secret.map(str::to_string),
        options,
    }
}

pub fn with_option(mut target: Target, key: &str, value: &str) -> Target {
    target.options.insert(key.to_string(), value.to_string());
    target
}
