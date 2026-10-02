use anyhow::Result;
use porthmos_ssh::Session;
use russh_sftp::client::SftpSession;

const SFTP_REQUEST_TIMEOUT_SECS: u64 = 60;
const SFTP_MAX_CONCURRENT_READS: usize = 8;
pub const SFTP_MAX_CONCURRENT_WRITES: usize = 16;
pub const SFTP_MAX_WRITE_PACKET_LEN: u32 = 32 * 1024;

pub async fn open_sftp(session: &Session) -> Result<SftpSession> {
    let channel = session.handle().channel_open_session().await?;
    channel.request_subsystem(true, "sftp").await?;
    let config = russh_sftp::client::Config {
        request_timeout_secs: SFTP_REQUEST_TIMEOUT_SECS,
        max_concurrent_reads: SFTP_MAX_CONCURRENT_READS,
        max_concurrent_writes: SFTP_MAX_CONCURRENT_WRITES,
        max_write_packet_len: SFTP_MAX_WRITE_PACKET_LEN,
        ..russh_sftp::client::Config::default()
    };
    Ok(SftpSession::new_with_config(channel.into_stream(), config).await?)
}
