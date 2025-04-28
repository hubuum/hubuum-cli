use config::Value;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Http,
    #[default]
    Https,
}

impl FromStr for Protocol {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "http" => Ok(Protocol::Http),
            "https" => Ok(Protocol::Https),
            _ => Err(format!("Invalid protocol: {}. Use 'http' or 'https'.", s)),
        }
    }
}

impl From<Protocol> for Value {
    fn from(val: Protocol) -> Self {
        Value::new(None, val.to_string())
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Protocol::Http => write!(f, "http"),
            Protocol::Https => write!(f, "https"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenEntry {
    pub hostname: String,
    pub username: String,
    pub token: String,
}
