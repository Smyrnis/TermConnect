#[cfg(not(unix))]
compile_error!("porthmos only supports Unix-like platforms (Linux/macOS)");

pub mod client;
mod connect;
mod discovery;
mod exec;
mod quote;
pub mod shell;
pub mod ssh_config;
#[cfg(feature = "testing")]
pub mod testing;

pub use connect::{ConnectOptions, Session, connect};
pub use discovery::{DEFAULT_PORT, discover, identity_path, shell_command};
pub use exec::{ExecChannel, ExecInput, Output, exec, open_exec};
pub use quote::shell_quote;
