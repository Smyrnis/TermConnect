pub const RESERVED_KEYS: [&str; 7] = ["protocol", "name", "host", "port", "username", "password", "remote_path"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonField {
    pub label: &'static str,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortField {
    pub label: &'static str,
    pub default: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub value: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionKind {
    Text { default: &'static str },
    Secret,
    Choice { choices: &'static [Choice], default: &'static str },
    Toggle { default: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionField {
    pub key: &'static str,
    pub label: &'static str,
    pub required: bool,
    pub kind: OptionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionForm {
    pub host: CommonField,
    pub port: PortField,
    pub username: CommonField,
    pub password: CommonField,
    pub options: Vec<OptionField>,
}

impl ConnectionForm {
    pub fn standard(default_port: u16) -> Self {
        Self {
            host: CommonField { label: "Host", required: true },
            port: PortField { label: "Port", default: default_port },
            username: CommonField { label: "Username", required: true },
            password: CommonField { label: "Password", required: false },
            options: Vec::new(),
        }
    }

    pub fn option(&self, key: &str) -> Option<&OptionField> {
        self.options.iter().find(|field| field.key == key)
    }

    pub fn reserved_key_collisions(&self) -> Vec<&'static str> {
        self.options.iter().map(|field| field.key).filter(|key| RESERVED_KEYS.contains(key)).collect()
    }
}

#[cfg(test)]
mod tests;
