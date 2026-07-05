use config::Value;
use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, str::FromStr};
use strum::{Display, EnumString};

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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Display, EnumString)]
pub enum OutputFormat {
    #[strum(serialize = "JSON")]
    Json,
    Text,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Display, Default)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum OutputColor {
    #[default]
    Auto,
    Always,
    Never,
}

impl FromStr for OutputColor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(OutputColor::Auto),
            "always" => Ok(OutputColor::Always),
            "never" => Ok(OutputColor::Never),
            _ => Err(format!(
                "Invalid output color: {s}. Use auto, always, or never."
            )),
        }
    }
}

impl From<OutputColor> for Value {
    fn from(val: OutputColor) -> Self {
        Value::new(None, val.to_string())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Display, Default)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum TableStyle {
    Ascii,
    Compact,
    Dense,
    Markdown,
    Plain,
    #[default]
    Rounded,
}

impl FromStr for TableStyle {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ascii" => Ok(TableStyle::Ascii),
            "compact" => Ok(TableStyle::Compact),
            "dense" => Ok(TableStyle::Dense),
            "markdown" => Ok(TableStyle::Markdown),
            "plain" => Ok(TableStyle::Plain),
            "rounded" => Ok(TableStyle::Rounded),
            _ => Err(format!(
                "Invalid table style: {s}. Use ascii, compact, dense, markdown, plain, or rounded."
            )),
        }
    }
}

impl From<TableStyle> for Value {
    fn from(val: TableStyle) -> Self {
        Value::new(None, val.to_string())
    }
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum TableWidth {
    #[default]
    Auto,
    Full,
    Fixed(u16),
}

impl<'de> Deserialize<'de> for TableWidth {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for TableWidth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Full => write!(f, "full"),
            Self::Fixed(width) => write!(f, "{width}"),
        }
    }
}

impl FromStr for TableWidth {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(TableWidth::Auto),
            "full" => Ok(TableWidth::Full),
            value => value
                .parse::<u16>()
                .map(TableWidth::Fixed)
                .map_err(|_| format!("Invalid table width: {s}. Use auto, full, or a number.")),
        }
    }
}

impl From<TableWidth> for Value {
    fn from(val: TableWidth) -> Self {
        Value::new(None, val.to_string())
    }
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum TableWrap {
    #[default]
    Auto,
    Never,
    Fixed(u16),
}

impl<'de> Deserialize<'de> for TableWrap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for TableWrap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Never => write!(f, "never"),
            Self::Fixed(width) => write!(f, "{width}"),
        }
    }
}

impl FromStr for TableWrap {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(TableWrap::Auto),
            "never" => Ok(TableWrap::Never),
            value => value
                .parse::<u16>()
                .map(TableWrap::Fixed)
                .map_err(|_| format!("Invalid table wrap: {s}. Use auto, never, or a number.")),
        }
    }
}

impl From<TableWrap> for Value {
    fn from(val: TableWrap) -> Self {
        Value::new(None, val.to_string())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Display, Default)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum EmptyResult {
    #[default]
    Message,
    Silent,
}

impl FromStr for EmptyResult {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "message" => Ok(EmptyResult::Message),
            "silent" => Ok(EmptyResult::Silent),
            _ => Err(format!(
                "Invalid empty result mode: {s}. Use message or silent."
            )),
        }
    }
}

impl From<EmptyResult> for Value {
    fn from(val: EmptyResult) -> Self {
        Value::new(None, val.to_string())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Display, Default)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum TableBands {
    #[default]
    Auto,
    Always,
    Never,
}

impl FromStr for TableBands {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(TableBands::Auto),
            "always" => Ok(TableBands::Always),
            "never" => Ok(TableBands::Never),
            _ => Err(format!(
                "Invalid table bands mode: {s}. Use auto, always, or never."
            )),
        }
    }
}

impl From<TableBands> for Value {
    fn from(val: TableBands) -> Self {
        Value::new(None, val.to_string())
    }
}
