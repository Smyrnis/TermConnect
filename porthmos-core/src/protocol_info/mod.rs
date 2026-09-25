use porthmos_vfs::{ConnectionForm, Protocol};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    pub form: ConnectionForm,
}

impl ProtocolInfo {
    pub fn from_protocol(protocol: &dyn Protocol) -> Self {
        Self { id: protocol.id(), display_name: protocol.display_name(), form: protocol.connection_form() }
    }
}

#[cfg(test)]
mod tests;
