use std::{fmt, str::FromStr};

use crate::SessionError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStorage {
    #[default]
    File,
    Keychain,
    None,
}

impl FromStr for SessionStorage {
    type Err = SessionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "file" => Ok(Self::File),
            "keychain" => Ok(Self::Keychain),
            "none" => Ok(Self::None),
            _ => Err(SessionError::InvalidStorage),
        }
    }
}

impl fmt::Display for SessionStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::File => "file",
            Self::Keychain => "keychain",
            Self::None => "none",
        })
    }
}
