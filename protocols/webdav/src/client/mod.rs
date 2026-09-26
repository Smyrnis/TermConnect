use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use reqwest::{
    Method, RequestBuilder, Response, StatusCode,
    header::{AUTHORIZATION, WWW_AUTHENTICATE},
};
use rustls::ClientConfig;

use crate::{
    auth::{Authenticator, Challenge, Scheme, challenges},
    errors::Failure,
    paths::Locator,
};

const KEEPALIVE_PROBES: u32 = 3;
const RESPONSE_WAIT_FACTOR: u32 = 4;
const LONG_WAIT_FACTOR: u32 = 20;

pub(crate) struct DavClient {
    http: reqwest::Client,
    timeout: Duration,
    locator: Locator,
    auth: Mutex<Authenticator>,
}

pub(crate) fn method(name: &'static str) -> Method {
    Method::from_bytes(name.as_bytes()).expect("WebDAV method names are valid tokens")
}

pub(crate) fn challenges_of(response: &Response) -> Vec<Challenge> {
    challenges(response.headers().get_all(WWW_AUTHENTICATE).iter().filter_map(|value| value.to_str().ok()))
}

impl DavClient {
    pub(crate) fn new(
        locator: Locator, tls: Option<Arc<ClientConfig>>, timeout: Duration,
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
        Ok(Self { http: builder.build()?, timeout, locator, auth: Mutex::default() })
    }

    pub(crate) fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) fn long_wait(&self) -> Duration {
        self.timeout * LONG_WAIT_FACTOR
    }

    pub(crate) fn awaits_challenge(&self) -> bool {
        self.authenticator().awaits_challenge()
    }

    pub(crate) fn locator(&self) -> &Locator {
        &self.locator
    }

    fn authenticator(&self) -> MutexGuard<'_, Authenticator> {
        self.auth.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn use_scheme(&self, scheme: Scheme) {
        self.authenticator().use_scheme(scheme);
    }

    pub(crate) fn use_credentials(&self, username: &str, password: &str) {
        self.authenticator().use_credentials(username, password);
    }

    pub(crate) fn request(&self, method: Method, href: &str) -> RequestBuilder {
        let header = self.authenticator().header(method.as_str(), href);
        let builder = self.http.request(method, self.locator.url(href));
        match header {
            Some(value) => builder.header(AUTHORIZATION, value),
            None => builder,
        }
    }

    pub(crate) fn refresh(&self, response: &Response) -> bool {
        self.authenticator().refresh(challenges_of(response))
    }

    pub(crate) async fn send(
        &self, method: Method, href: &str, prepare: impl Fn(RequestBuilder) -> RequestBuilder,
    ) -> Result<Response, Failure> {
        let response = self.answer(&method, prepare(self.request(method.clone(), href))).await?;
        if response.status() == StatusCode::UNAUTHORIZED && self.refresh(&response) {
            return self.answer(&method, prepare(self.request(method.clone(), href))).await;
        }
        Ok(response)
    }

    async fn answer(&self, method: &Method, request: RequestBuilder) -> Result<Response, Failure> {
        let limit = if waits_on_the_server(method) { self.long_wait() } else { self.timeout * RESPONSE_WAIT_FACTOR };
        match tokio::time::timeout(limit, request.send()).await {
            Ok(result) => Ok(result?),
            Err(_) => Err(Failure::TimedOut),
        }
    }
}

fn waits_on_the_server(method: &Method) -> bool {
    matches!(method.as_str(), "PUT" | "PATCH" | "MOVE" | "COPY" | "DELETE")
}
