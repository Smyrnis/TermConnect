use std::{net::IpAddr, sync::Arc, time::Duration};

use anyhow::anyhow;
use porthmos_vfs::{ErrorKind, ProtocolError};
use rustls::ClientConfig;
use suppaftp::{
    FtpError, Mode, Status,
    tokio::{AsyncRustlsConnector, AsyncRustlsFtpStream},
    types::FileType,
};

use porthmos_tls::{ProblemSlot, TrustProblem};

use crate::{
    errors::ftp_error,
    settings::{FtpSettings, Security},
};

pub(crate) type Connection = AsyncRustlsFtpStream;

const IMPLICIT_PORT_HINT: &str = " — implicit TLS usually uses port 990";
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) async fn bounded<T>(
    limit: Duration, future: impl std::future::Future<Output = Result<T, ProtocolError>>,
) -> Result<T, ProtocolError> {
    tokio::time::timeout(limit, future)
        .await
        .unwrap_or_else(|_| Err(ProtocolError::new(ErrorKind::Connect, anyhow!("timed out logging in"))))
}

pub(crate) enum Login {
    Accepted,
    Rejected(String),
}

pub(crate) fn uses_nat_workaround(peer: IpAddr) -> bool {
    match peer {
        IpAddr::V4(address) => !(address.is_private() || address.is_loopback() || address.is_link_local()),
        IpAddr::V6(address) => !(address.is_loopback() || address.is_unique_local() || address.is_unicast_link_local()),
    }
}

async fn peer_address(host: &str, port: u16) -> Option<IpAddr> {
    tokio::net::lookup_host((host, port)).await.ok()?.next().map(|address| address.ip())
}

#[derive(Clone)]
pub(crate) struct SessionContext {
    pub(crate) settings: FtpSettings,
    pub(crate) tls: Arc<ClientConfig>,
    pub(crate) problem: ProblemSlot,
    pub(crate) timeout: Duration,
}

pub(crate) enum OpenError {
    Trust(TrustProblem),
    Other(ProtocolError),
}

pub(crate) fn with_port_hint(error: ProtocolError, security: Security, port: u16) -> ProtocolError {
    if security == Security::Implicit && port == 21 {
        return ProtocolError::new(error.kind(), anyhow!("{error}{IMPLICIT_PORT_HINT}"));
    }
    error
}

fn connector(tls: &Arc<ClientConfig>) -> AsyncRustlsConnector {
    AsyncRustlsConnector::from(tokio_rustls::TlsConnector::from(tls.clone()))
}

pub(crate) async fn open(context: &SessionContext) -> Result<Connection, OpenError> {
    let settings = &context.settings;
    context.problem.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    let address = (settings.host.as_str(), settings.port);
    let connecting = async {
        match settings.security {
            Security::Plain => Connection::connect(address).await,
            Security::Explicit => match Connection::connect(address).await {
                Ok(connection) => connection.into_secure(connector(&context.tls), &settings.host).await,
                Err(err) => Err(err),
            },
            Security::Implicit => {
                Connection::connect_secure_implicit(address, connector(&context.tls), &settings.host).await
            }
        }
    };
    let result = tokio::time::timeout(context.timeout, connecting).await.unwrap_or_else(|_| {
        Err(suppaftp::FtpError::ConnectionError(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("timed out connecting to {}:{}", settings.host, settings.port),
        )))
    });
    result.map_err(|err| {
        let problem = context.problem.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
        match problem {
            Some(problem) => OpenError::Trust(problem),
            None => OpenError::Other(with_port_hint(
                ProtocolError::new(ErrorKind::Connect, anyhow!(err)),
                settings.security,
                settings.port,
            )),
        }
    })
}

pub(crate) async fn login(connection: &mut Connection, username: &str, password: &str) -> Result<Login, ProtocolError> {
    match connection.login(username, password).await {
        Ok(()) => Ok(Login::Accepted),
        Err(FtpError::UnexpectedResponse(response)) if response.status == Status::NotLoggedIn => {
            Ok(Login::Rejected(String::from_utf8_lossy(&response.body).trim().to_string()))
        }
        Err(err) => Err(ftp_error(err)),
    }
}

pub(crate) async fn prepare(connection: &mut Connection, settings: &FtpSettings) -> Result<(), ProtocolError> {
    connection.transfer_type(FileType::Binary).await.map_err(ftp_error)?;
    let _ = connection.opts("UTF8", Some("ON")).await;
    connection.set_mode(if settings.passive { Mode::Passive } else { Mode::Active });
    if settings.passive && peer_address(&settings.host, settings.port).await.is_some_and(uses_nat_workaround) {
        connection.set_passive_nat_workaround(true);
    }
    Ok(())
}

pub(crate) async fn open_logged_in(context: &SessionContext, password: &str) -> Result<Connection, ProtocolError> {
    let mut connection = open(context).await.map_err(|err| match err {
        OpenError::Trust(_) => ProtocolError::new(ErrorKind::Connect, anyhow!("the server certificate is not trusted")),
        OpenError::Other(err) => err,
    })?;
    let logging_in = async {
        if let Login::Rejected(reply) = login(&mut connection, &context.settings.username, password).await? {
            return Err(ProtocolError::new(ErrorKind::AuthRejected, anyhow!("the server rejected the login: {reply}")));
        }
        prepare(&mut connection, &context.settings).await
    };
    bounded(context.timeout, logging_in).await?;
    Ok(connection)
}

#[cfg(test)]
mod tests;
