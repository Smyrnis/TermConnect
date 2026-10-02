use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    Paths,
    persist::{Loaded, read_or_set_aside, toml_problem, write_atomic},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    pub save_passwords_in_keyring: bool,
}

pub struct StateLoad {
    pub state: UiState,
    pub warning: Option<String>,
    pub protected: bool,
}

pub fn load(paths: &Paths) -> StateLoad {
    match read_or_set_aside(&paths.state_file(), "interface state", |text| {
        toml::from_str::<UiState>(text).map_err(|err| toml_problem(text, &err))
    }) {
        Loaded::Missing => StateLoad { state: UiState::default(), warning: None, protected: false },
        Loaded::Ready(state) => StateLoad { state, warning: None, protected: false },
        Loaded::SetAside { warning, protected } => {
            StateLoad { state: UiState::default(), warning: Some(warning), protected }
        }
    }
}

pub fn save(paths: &Paths, state: &UiState) -> Result<()> {
    write_atomic(&paths.state_file(), toml::to_string(state)?.as_bytes(), 0o600)
}

#[cfg(test)]
mod tests;
