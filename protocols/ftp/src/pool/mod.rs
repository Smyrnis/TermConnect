use std::{
    future::Future,
    io,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use porthmos_vfs::ProtocolError;
use suppaftp::{FtpError, FtpResult, Status};
use tokio::sync::Mutex;

use crate::{
    errors::ftp_error,
    session::{Connection, SessionContext, open_logged_in},
};

pub(crate) type ConnectionFuture<'c, T> = Pin<Box<dyn Future<Output = FtpResult<T>> + Send + 'c>>;

pub(crate) const LISTING_TIMEOUT: Duration = Duration::from_secs(3_600);

pub(crate) struct Attempt<T> {
    pub(crate) result: FtpResult<T>,
    pub(crate) replayed: bool,
}

pub(crate) struct Pool {
    context: SessionContext,
    password: String,
    main: Mutex<Connection>,
    idle: std::sync::Mutex<Vec<Connection>>,
    mlst_missing: AtomicBool,
}

pub(crate) fn timed_out() -> FtpError {
    FtpError::ConnectionError(io::Error::new(io::ErrorKind::TimedOut, "timed out waiting for the server"))
}

pub(crate) fn is_connection_lost(err: &FtpError) -> bool {
    match err {
        FtpError::ConnectionError(_) | FtpError::BadResponse | FtpError::SecureError(_) => true,
        FtpError::UnexpectedResponse(response) => matches!(response.status, Status::NotAvailable | Status::Closing),
        _ => false,
    }
}

pub(crate) fn is_not_implemented(err: &FtpError) -> bool {
    matches!(
        err,
        FtpError::UnexpectedResponse(response)
            if matches!(response.status, Status::BadCommand | Status::NotImplemented | Status::NotImplementedParameter)
    )
}

pub(crate) async fn within<T>(limit: Duration, future: impl Future<Output = FtpResult<T>>) -> FtpResult<T> {
    tokio::time::timeout(limit, future).await.unwrap_or_else(|_| Err(timed_out()))
}

impl Pool {
    pub(crate) fn new(context: SessionContext, password: String, main: Connection) -> Self {
        Self {
            context,
            password,
            main: Mutex::new(main),
            idle: std::sync::Mutex::default(),
            mlst_missing: AtomicBool::new(false),
        }
    }

    pub(crate) fn timeout(&self) -> Duration {
        self.context.timeout
    }

    pub(crate) fn supports_mlst(&self) -> bool {
        !self.mlst_missing.load(Ordering::Relaxed)
    }

    pub(crate) fn mark_mlst_missing(&self) {
        self.mlst_missing.store(true, Ordering::Relaxed);
    }

    pub(crate) async fn attempt<T>(
        &self, limit: Duration, op: impl for<'c> Fn(&'c mut Connection) -> ConnectionFuture<'c, T>,
    ) -> Result<Attempt<T>, ProtocolError> {
        let mut main = self.main.lock().await;
        match within(limit, op(&mut main)).await {
            Err(FtpError::BadResponse) if within(self.context.timeout, main.noop()).await.is_ok() => {
                Ok(Attempt { result: Err(FtpError::BadResponse), replayed: false })
            }
            Err(err) if is_connection_lost(&err) => {
                *main = open_logged_in(&self.context, &self.password).await?;
                Ok(Attempt { result: within(limit, op(&mut main)).await, replayed: true })
            }
            result => Ok(Attempt { result, replayed: false }),
        }
    }

    pub(crate) async fn run<T>(
        &self, op: impl for<'c> Fn(&'c mut Connection) -> ConnectionFuture<'c, T>,
    ) -> Result<T, ProtocolError> {
        self.attempt(self.context.timeout, op).await?.result.map_err(ftp_error)
    }

    pub(crate) async fn run_raw<T>(
        &self, op: impl for<'c> Fn(&'c mut Connection) -> ConnectionFuture<'c, T>,
    ) -> Result<FtpResult<T>, ProtocolError> {
        Ok(self.attempt(self.context.timeout, op).await?.result)
    }

    pub(crate) async fn run_raw_listing<T>(
        &self, op: impl for<'c> Fn(&'c mut Connection) -> ConnectionFuture<'c, T>,
    ) -> Result<FtpResult<T>, ProtocolError> {
        Ok(self.attempt(LISTING_TIMEOUT, op).await?.result)
    }

    pub(crate) async fn borrow(&self) -> Result<Connection, ProtocolError> {
        loop {
            let idle = self.idle.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).pop();
            let Some(mut connection) = idle else {
                return open_logged_in(&self.context, &self.password).await;
            };
            if within(self.context.timeout, connection.noop()).await.is_ok() {
                return Ok(connection);
            }
        }
    }

    pub(crate) fn give_back(&self, connection: Connection) {
        self.idle.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(connection);
    }
}

#[cfg(test)]
mod tests;
