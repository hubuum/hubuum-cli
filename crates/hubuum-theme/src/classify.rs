use std::net::IpAddr;

use chrono::{DateTime, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::ThemeRole;

/// Semantic categories for complete field values, independent of their styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueCategory {
    Number,
    Ipv4,
    Ipv6,
    BooleanTrue,
    BooleanFalse,
    NullLike,
    Command,
    Cidr,
    Date,
    Url,
    Mac,
    Ordinary,
}

impl ValueCategory {
    pub const fn role(self) -> ThemeRole {
        match self {
            Self::Number => ThemeRole::Number,
            Self::Ipv4 => ThemeRole::Ipv4,
            Self::Ipv6 => ThemeRole::Ipv6,
            Self::BooleanTrue => ThemeRole::BooleanTrue,
            Self::BooleanFalse => ThemeRole::BooleanFalse,
            Self::NullLike => ThemeRole::NullLike,
            Self::Command => ThemeRole::Command,
            Self::Cidr => ThemeRole::Cidr,
            Self::Date => ThemeRole::Date,
            Self::Url => ThemeRole::Url,
            Self::Mac => ThemeRole::Mac,
            Self::Ordinary => ThemeRole::Value,
        }
    }
}

/// Classifier settings shared by all renderers using a theme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ValueClassifier {
    disabled: Vec<ValueCategory>,
    command_prefixes: Vec<CommandPrefix>,
}

impl Default for ValueClassifier {
    fn default() -> Self {
        Self {
            disabled: Vec::new(),
            command_prefixes: vec![CommandPrefix("hubuum-cli".to_string())],
        }
    }
}

impl ValueClassifier {
    /// Classifies trimmed text. Rendering must retain the original value.
    /// Disabling a category makes its values ordinary; it does not reinterpret them.
    pub fn classify(&self, text: &str) -> ValueCategory {
        let category = self.recognize(text.trim());
        if self.disabled.contains(&category) {
            ValueCategory::Ordinary
        } else {
            category
        }
    }

    fn recognize(&self, text: &str) -> ValueCategory {
        if text.is_empty() {
            return ValueCategory::Ordinary;
        }
        if text.eq_ignore_ascii_case("true") {
            return ValueCategory::BooleanTrue;
        }
        if text.eq_ignore_ascii_case("false") {
            return ValueCategory::BooleanFalse;
        }
        if ["null", "none", "nil", "n/a"]
            .iter()
            .any(|null| text.eq_ignore_ascii_case(null))
        {
            return ValueCategory::NullLike;
        }
        // Validate syntax without converting to a machine-sized numeric value.
        if is_number(text) {
            return ValueCategory::Number;
        }
        if let Ok(ip) = text.parse::<IpAddr>() {
            return match ip {
                IpAddr::V4(_) => ValueCategory::Ipv4,
                IpAddr::V6(_) => ValueCategory::Ipv6,
            };
        }
        if is_cidr(text) {
            return ValueCategory::Cidr;
        }
        if is_date(text) {
            return ValueCategory::Date;
        }
        if is_url(text) {
            return ValueCategory::Url;
        }
        if is_mac(text) {
            return ValueCategory::Mac;
        }
        if self.command_prefixes.iter().any(|prefix| {
            text.strip_prefix(&prefix.0)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        }) {
            return ValueCategory::Command;
        }
        ValueCategory::Ordinary
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
struct CommandPrefix(String);

impl TryFrom<String> for CommandPrefix {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() || value.trim() != value || value.chars().any(char::is_control) {
            return Err(
                "command prefixes must be nonempty, trimmed text without control characters",
            );
        }
        Ok(Self(value))
    }
}

impl From<CommandPrefix> for String {
    fn from(prefix: CommandPrefix) -> Self {
        prefix.0
    }
}

fn is_number(text: &str) -> bool {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let mut digits = unsigned.bytes().take_while(u8::is_ascii_digit).count();
    let mut rest = &unsigned[digits..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let count = fraction.bytes().take_while(u8::is_ascii_digit).count();
        digits += count;
        rest = &fraction[count..];
    }
    if digits == 0 {
        return false;
    }
    if let Some(exponent) = rest.strip_prefix(['e', 'E']) {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        return !exponent.is_empty() && exponent.bytes().all(|byte| byte.is_ascii_digit());
    }
    rest.is_empty()
}

fn is_cidr(text: &str) -> bool {
    let Some((address, prefix)) = text.split_once('/') else {
        return false;
    };
    if prefix.is_empty() || !prefix.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    match (address.parse::<IpAddr>(), prefix.parse::<u8>()) {
        (Ok(IpAddr::V4(_)), Ok(prefix)) => prefix <= 32,
        (Ok(IpAddr::V6(_)), Ok(prefix)) => prefix <= 128,
        _ => false,
    }
}

fn is_date(text: &str) -> bool {
    (text.len() == 10
        && text.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        })
        && NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok())
        || DateTime::parse_from_rfc3339(text).is_ok()
        || is_local_timestamp(text)
}

fn is_local_timestamp(text: &str) -> bool {
    text.len() >= 19
        && text
            .bytes()
            .take(19)
            .enumerate()
            .all(|(index, byte)| match index {
                4 | 7 => byte == b'-',
                10 => byte == b'T',
                13 | 16 => byte == b':',
                _ => byte.is_ascii_digit(),
            })
        && (text.len() == 19
            || text[19..].strip_prefix('.').is_some_and(|fraction| {
                !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
            }))
        && NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").is_ok()
}

fn is_url(text: &str) -> bool {
    // Require an explicit hierarchical URL; bare hosts and MACs are not schemes.
    text.contains("://")
        && !text
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        && Url::parse(text).is_ok_and(|url| url.has_host() || url.scheme() == "file")
}

fn is_mac(text: &str) -> bool {
    let (separator, groups, width) = if text.contains(':') {
        (':', 6, 2)
    } else if text.contains('-') {
        ('-', 6, 2)
    } else {
        ('.', 3, 4)
    };
    let mut count = 0;
    text.split(separator).all(|group| {
        count += 1;
        group.len() == width && group.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) && count == groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_complete_values() {
        let classifier = ValueClassifier::default();
        for (category, values) in [
            (
                ValueCategory::Number,
                vec![
                    "0",
                    "-42",
                    "+3.5",
                    ".5",
                    "1.",
                    "6.02e23",
                    "-2E-3",
                    "1e9999",
                    "999999999999999999999999999999",
                ],
            ),
            (ValueCategory::Ipv4, vec!["192.168.0.1", "0.0.0.0"]),
            (
                ValueCategory::Ipv6,
                vec!["2001:db8::1", "::", "::ffff:192.0.2.1"],
            ),
            (ValueCategory::BooleanTrue, vec!["true", "TRUE", "TrUe"]),
            (ValueCategory::BooleanFalse, vec!["false", "FALSE"]),
            (ValueCategory::NullLike, vec!["null", "NONE", "Nil", "N/a"]),
            (
                ValueCategory::Command,
                vec!["hubuum-cli", "hubuum-cli object list"],
            ),
            (
                ValueCategory::Cidr,
                vec!["192.168.0.1/24", "0.0.0.0/0", "2001:db8::/32", "::1/128"],
            ),
            (
                ValueCategory::Date,
                vec![
                    "2024-02-29",
                    "2026-10-26T12:34:56Z",
                    "2026-10-26T12:34:56.123+01:00",
                    "2020-07-30T11:46:02",
                    "2020-07-30T11:46:02.123456",
                ],
            ),
            (
                ValueCategory::Url,
                vec![
                    "https://example.org/path?q=1",
                    "ssh://host/path",
                    "file:///tmp/data",
                    "http://[::1]:8080",
                ],
            ),
            (
                ValueCategory::Mac,
                vec!["00:1A:2b:3C:4d:5E", "00-1a-2b-3c-4d-5e", "001a.2b3c.4d5e"],
            ),
        ] {
            for value in values {
                assert_eq!(classifier.classify(value), category, "{value}");
                assert_eq!(
                    classifier.classify(&format!(" \t{value}\n")),
                    category,
                    "{value}"
                );
            }
        }
    }

    #[test]
    fn rejects_partial_ambiguous_and_invalid_values() {
        for value in [
            "",
            " ",
            "NaN",
            "inf",
            "-Infinity",
            "1e",
            "1e+",
            "--1",
            "+-1",
            ".",
            "0x42",
            "10 MB",
            "1,000",
            "host.example",
            "192.168.1.999",
            "1.2.3",
            "2001:xyz::1",
            "192.168.0.1/33",
            "::/129",
            "::/-1",
            "::/+1",
            "::/",
            "2023-02-29",
            "2026-13-01",
            "2026-1-01",
            "01/02/2026",
            "2026-10-26T25:00:00Z",
            "2020-07-30T25:46:02",
            "2023-02-29T11:46:02",
            "2020-07-30T11:46",
            "2020-07-30T11:46:02junk",
            "2020-07-30T11:46:02.",
            "2020-07-30T1:46:02.1",
            "https://",
            "https://host/with space",
            "www.example.org",
            "00:11:22:33:44",
            "00:11:22:33:44:gg",
            "00:11-22:33:44:55",
            "aa1122334455",
            "hubuum-client",
            "osp list",
            "true story",
        ] {
            assert_eq!(
                ValueClassifier::default().classify(value),
                ValueCategory::Ordinary,
                "{value}"
            );
        }
    }

    #[test]
    fn settings_are_validated_and_tunable() {
        let classifier: ValueClassifier =
            toml::from_str("disabled = ['number', 'date']\ncommand_prefixes = ['osp']").unwrap();
        assert_eq!(classifier.classify("42"), ValueCategory::Ordinary);
        assert_eq!(classifier.classify("2024-02-29"), ValueCategory::Ordinary);
        assert_eq!(classifier.classify("osp list"), ValueCategory::Command);
        assert_eq!(
            classifier.classify("hubuum-cli list"),
            ValueCategory::Ordinary
        );
        for invalid in [
            "command_prefixes = ['']",
            "command_prefixes = [' osp']",
            "disabled = ['muted']",
            "disable = ['number']",
        ] {
            assert!(
                toml::from_str::<ValueClassifier>(invalid).is_err(),
                "{invalid}"
            );
        }
    }
}
