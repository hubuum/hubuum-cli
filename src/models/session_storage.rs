use std::str::FromStr;

use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum SessionStorage {
    #[default]
    File,
    Keychain,
}

impl FromStr for SessionStorage {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "file" => Ok(Self::File),
            "keychain" => Ok(Self::Keychain),
            _ => Err("Session storage must be 'file' or 'keychain'".into()),
        }
    }
}
