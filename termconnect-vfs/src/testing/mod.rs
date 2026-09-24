mod fake_fs;
mod fake_protocol;

pub use fake_fs::FakeFs;
pub use fake_protocol::FakeProtocol;

#[cfg(test)]
mod tests;
