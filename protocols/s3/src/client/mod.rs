use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use bytes::Bytes;
use futures_util::TryStreamExt;
use porthmos_vfs::ProtocolError;
use reqwest::{
    Method, Response, StatusCode,
    header::{AUTHORIZATION, DATE},
};
use rustls::ClientConfig;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::io::StreamReader;

use crate::{
    clock,
    errors::{Failure, S3Error},
    idle::IdleReader,
    locate::Endpoint,
    settings::{Addressing, S3Settings},
    sign::{self, Credentials, Signable},
    xml,
};

const KEEPALIVE_PROBES: u32 = 3;
const RESPONSE_WAIT_FACTOR: u32 = 4;
const LONG_WAIT_FACTOR: u32 = 20;
const REGION_HEADER: &str = "x-amz-bucket-region";
const SLOWEST_UPLOAD_BYTES_PER_SECOND: u64 = 16 * 1024;

pub(crate) fn useful_hint(hint: Option<String>, region: &str) -> Option<String> {
    hint.filter(|hinted| !hinted.is_empty() && hinted != region)
}

pub(crate) fn reply_limit(method: &Method, body_len: usize, timeout: Duration) -> Duration {
    if matches!(*method, Method::GET | Method::HEAD) {
        return timeout * RESPONSE_WAIT_FACTOR;
    }
    timeout * LONG_WAIT_FACTOR + Duration::from_secs(body_len as u64 / SLOWEST_UPLOAD_BYTES_PER_SECOND)
}

#[derive(Clone)]
pub(crate) struct Call<'a> {
    method: Method,
    bucket: Option<&'a str>,
    key: &'a str,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Bytes,
}

impl<'a> Call<'a> {
    pub(crate) fn new(method: Method, bucket: Option<&'a str>, key: &'a str) -> Self {
        Self { method, bucket, key, query: Vec::new(), headers: Vec::new(), body: Bytes::new() }
    }

    pub(crate) fn query(mut self, name: &str, value: &str) -> Self {
        self.query.push((name.to_string(), value.to_string()));
        self
    }

    pub(crate) fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_ascii_lowercase(), value.to_string()));
        self
    }

    pub(crate) fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = body.into();
        self
    }
}

pub(crate) struct S3Client {
    http: reqwest::Client,
    settings: S3Settings,
    endpoint: Endpoint,
    timeout: Duration,
    region: Mutex<String>,
    bucket_regions: Mutex<HashMap<String, String>>,
    secret: Mutex<String>,
    skew: Mutex<i64>,
}

impl S3Client {
    pub(crate) fn new(
        settings: &S3Settings, tls: Option<Arc<ClientConfig>>, timeout: Duration,
    ) -> Result<Self, reqwest::Error> {
        let mut builder = reqwest::Client::builder()
            .connect_timeout(timeout)
            .tcp_keepalive(timeout)
            .tcp_keepalive_interval(timeout / KEEPALIVE_PROBES)
            .tcp_keepalive_retries(KEEPALIVE_PROBES)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none());
        #[cfg(target_os = "linux")]
        {
            builder = builder.tcp_user_timeout(timeout * RESPONSE_WAIT_FACTOR);
        }
        if let Some(config) = tls {
            builder = builder.use_preconfigured_tls(ClientConfig::clone(&config));
        }
        Ok(Self {
            http: builder.build()?,
            settings: settings.clone(),
            endpoint: Endpoint::new(settings.secure, &settings.endpoint, settings.port),
            timeout,
            region: Mutex::new(settings.region.clone()),
            bucket_regions: Mutex::default(),
            secret: Mutex::new(settings.secret.clone().unwrap_or_default()),
            skew: Mutex::new(0),
        })
    }

    pub(crate) fn region(&self) -> String {
        self.region.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub(crate) fn set_region(&self, region: &str) {
        *self.region.lock().unwrap_or_else(PoisonError::into_inner) = region.to_string();
    }

    pub(crate) fn skew(&self) -> i64 {
        *self.skew.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn region_for(&self, bucket: Option<&str>) -> String {
        bucket
            .and_then(|bucket| self.bucket_regions.lock().unwrap_or_else(PoisonError::into_inner).get(bucket).cloned())
            .unwrap_or_else(|| self.region())
    }

    pub(crate) fn set_secret(&self, secret: &str) {
        *self.secret.lock().unwrap_or_else(PoisonError::into_inner) = secret.to_string();
    }

    fn credentials(&self) -> Credentials {
        Credentials {
            access_key: self.settings.access_key.clone(),
            secret: self.secret.lock().unwrap_or_else(PoisonError::into_inner).clone(),
        }
    }

    pub(crate) async fn send(&self, call: Call<'_>) -> Result<Response, Failure> {
        let region = self.region_for(call.bucket);
        let response = self.attempt(call.clone(), &region).await?;
        let Some(bucket) = call.bucket else {
            return Ok(response);
        };
        if !matches!(response.status(), StatusCode::MOVED_PERMANENTLY | StatusCode::BAD_REQUEST) {
            return Ok(response);
        }
        let (response, hint) = self.region_hint(response).await;
        let Some(hinted) = useful_hint(hint, &region) else {
            return Ok(response);
        };
        let retried = self.attempt(call, &hinted).await?;
        if !matches!(retried.status(), StatusCode::MOVED_PERMANENTLY | StatusCode::BAD_REQUEST) {
            self.bucket_regions.lock().unwrap_or_else(PoisonError::into_inner).insert(bucket.to_string(), hinted);
        }
        Ok(retried)
    }

    async fn region_hint(&self, response: Response) -> (Response, Option<String>) {
        if let Some(region) = response.headers().get(REGION_HEADER).and_then(|value| value.to_str().ok()) {
            let region = region.to_string();
            return (response, Some(region));
        }
        let status = response.status();
        let headers = response.headers().clone();
        let body = self.text(response).await.unwrap_or_default();
        let region = xml::error(status.as_u16(), &body).region;
        let mut rebuilt = http::Response::new(body);
        *rebuilt.status_mut() = status;
        *rebuilt.headers_mut() = headers;
        (Response::from(rebuilt), region)
    }

    async fn attempt(&self, call: Call<'_>, region: &str) -> Result<Response, Failure> {
        let addressing = call.bucket.map_or(Addressing::Path, |bucket| self.settings.addressing_for(bucket));
        let host = self.endpoint.host(call.bucket, addressing);
        let path = self.endpoint.path(call.bucket, call.key, addressing);
        let url = self.endpoint.url(&host, &path, &sign::canonical_query(&call.query));
        let (_, stamp) = clock::amz_date(clock::now());
        let hash = sign::payload_hash(&call.body);
        let mut headers = vec![
            ("host".to_string(), host),
            ("x-amz-date".to_string(), stamp.clone()),
            ("x-amz-content-sha256".to_string(), hash.clone()),
        ];
        headers.extend(call.headers);
        let signable = Signable {
            method: call.method.as_str(),
            path: &path,
            query: &call.query,
            headers: &headers,
            payload_hash: &hash,
        };
        let authorization = sign::authorization(&signable, &self.credentials(), region, "s3", &stamp);
        let mut request = self.http.request(call.method.clone(), url);
        for (name, value) in headers.iter().filter(|(name, _)| name != "host") {
            request = request.header(name.as_str(), value.as_str());
        }
        let limit = reply_limit(&call.method, call.body.len(), self.timeout);
        let request = request.header(AUTHORIZATION, authorization).body(call.body);
        let response = match tokio::time::timeout(limit, request.send()).await {
            Ok(result) => result?,
            Err(_) => return Err(Failure::TimedOut),
        };
        let server_date = response.headers().get(DATE).and_then(|value| value.to_str().ok());
        if server_date.is_some() {
            *self.skew.lock().unwrap_or_else(PoisonError::into_inner) = clock::skew(clock::now(), server_date);
        }
        Ok(response)
    }

    pub(crate) fn reader(&self, response: Response) -> IdleReader<impl AsyncRead + Unpin + use<>> {
        let stream = response.bytes_stream().map_err(|err| std::io::Error::other(err.without_url()));
        IdleReader::new(StreamReader::new(Box::pin(stream)), self.timeout)
    }

    pub(crate) async fn text(&self, response: Response) -> Result<String, ProtocolError> {
        let mut body = Vec::new();
        self.reader(response).read_to_end(&mut body).await?;
        Ok(String::from_utf8_lossy(&body).into_owned())
    }

    pub(crate) async fn error(&self, response: Response) -> S3Error {
        let status = response.status().as_u16();
        let header_region =
            response.headers().get(REGION_HEADER).and_then(|value| value.to_str().ok()).map(str::to_string);
        let body = self.text(response).await.unwrap_or_default();
        let mut error = xml::error(status, &body);
        error.region = error.region.or(header_region);
        error
    }
}

#[cfg(test)]
mod tests;
