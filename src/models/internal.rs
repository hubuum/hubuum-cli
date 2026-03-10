use config::Value;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use strum::{Display, EnumString};

use crate::{
    domain::{ResolvedClassRelationRecord, ResolvedObjectRelationRecord},
    errors::AppError,
    formatting::OutputFormatter,
};

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
            _ => Err(format!("Invalid protocol: {s}. Use 'http' or 'https'.")),
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Display, EnumString)]
pub enum OutputFormat {
    #[strum(serialize = "JSON")]
    Json,
    Text,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Display, Default)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum TableStyle {
    Ascii,
    Compact,
    Markdown,
    #[default]
    Rounded,
}

impl FromStr for TableStyle {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ascii" => Ok(TableStyle::Ascii),
            "compact" => Ok(TableStyle::Compact),
            "markdown" => Ok(TableStyle::Markdown),
            "rounded" => Ok(TableStyle::Rounded),
            _ => Err(format!(
                "Invalid table style: {s}. Use ascii, compact, markdown, or rounded."
            )),
        }
    }
}

impl From<TableStyle> for Value {
    fn from(val: TableStyle) -> Self {
        Value::new(None, val.to_string())
    }
}

pub enum Relation {
    Class(ResolvedClassRelationRecord),
    Object(ResolvedObjectRelationRecord),
}

impl Relation {
    pub fn format_json_noreturn(&self) -> Result<(), AppError> {
        match self {
            Relation::Class(r) => r.format_json_noreturn(),
            Relation::Object(r) => r.format_json_noreturn(),
        }
    }

    pub fn format_noreturn(&self) -> Result<(), AppError> {
        match self {
            Relation::Class(r) => r.format_noreturn(),
            Relation::Object(r) => r.format_noreturn(),
        }
    }
}

impl From<ResolvedClassRelationRecord> for Relation {
    fn from(r: ResolvedClassRelationRecord) -> Self {
        Relation::Class(r)
    }
}

impl From<ResolvedObjectRelationRecord> for Relation {
    fn from(r: ResolvedObjectRelationRecord) -> Self {
        Relation::Object(r)
    }
}
