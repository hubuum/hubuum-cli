use std::cmp::Ordering;
use std::net::IpAddr;

use chrono::{DateTime, Utc};
use semver::Version;
use serde_json::Value;

use crate::predicate::ValueCast;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CastValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Ip(IpAddr),
    DateTime(DateTime<Utc>),
    Version(Version),
    Natural(String),
}

impl CastValue {
    pub(crate) fn compare(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::String(left), Self::String(right)) => Some(left.cmp(right)),
            (Self::Number(left), Self::Number(right)) => left.partial_cmp(right),
            (Self::Boolean(left), Self::Boolean(right)) => Some(left.cmp(right)),
            (Self::Ip(left), Self::Ip(right)) => Some(compare_ip(left, right)),
            (Self::DateTime(left), Self::DateTime(right)) => Some(left.cmp(right)),
            (Self::Version(left), Self::Version(right)) => Some(left.cmp(right)),
            (Self::Natural(left), Self::Natural(right)) => Some(compare_natural(left, right)),
            _ => None,
        }
    }
}

pub(crate) fn cast_value(value: &Value, cast: ValueCast) -> Result<Option<CastValue>, String> {
    if value.is_null() {
        return Ok(None);
    }

    let casted = match cast {
        ValueCast::String => CastValue::String(
            scalar_cast_text(value)
                .ok_or_else(|| "expected a JSON string, number, or boolean".to_string())?,
        ),
        ValueCast::Number => CastValue::Number(parse_number(value)?),
        ValueCast::Boolean => CastValue::Boolean(parse_boolean(value)?),
        ValueCast::Ip => {
            CastValue::Ip(parse_string(value, "an IP address")?.parse().map_err(|_| {
                "expected an IPv4 or IPv6 address accepted by std::net::IpAddr".to_string()
            })?)
        }
        ValueCast::DateTime => {
            let parsed = DateTime::parse_from_rfc3339(parse_string(value, "an RFC 3339 datetime")?)
                .map_err(|_| "expected an RFC 3339 datetime with an explicit offset".to_string())?;
            CastValue::DateTime(parsed.with_timezone(&Utc))
        }
        ValueCast::Version => CastValue::Version(
            Version::parse(parse_string(value, "a semantic version")?)
                .map_err(|_| "expected a Semantic Versioning 2.0 value".to_string())?,
        ),
        ValueCast::Natural => {
            CastValue::Natural(parse_string(value, "a string for natural ordering")?.to_string())
        }
    };
    Ok(Some(casted))
}

fn parse_string<'a>(value: &'a Value, expected: &str) -> Result<&'a str, String> {
    value.as_str().ok_or_else(|| format!("expected {expected}"))
}

fn parse_number(value: &Value) -> Result<f64, String> {
    let number = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| value.as_f64()),
        _ => None,
    }
    .ok_or_else(|| "expected one complete finite JSON number".to_string())?;

    if number.is_finite() {
        Ok(number)
    } else {
        Err("expected one complete finite JSON number".to_string())
    }
}

fn parse_boolean(value: &Value) -> Result<bool, String> {
    match value {
        Value::Bool(value) => Ok(*value),
        Value::String(value) if value.eq_ignore_ascii_case("true") => Ok(true),
        Value::String(value) if value.eq_ignore_ascii_case("false") => Ok(false),
        _ => Err("expected true or false".to_string()),
    }
}

fn scalar_cast_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

fn compare_ip(left: &IpAddr, right: &IpAddr) -> Ordering {
    match (left, right) {
        (IpAddr::V4(left), IpAddr::V4(right)) => u32::from(*left).cmp(&u32::from(*right)),
        (IpAddr::V6(left), IpAddr::V6(right)) => u128::from(*left).cmp(&u128::from(*right)),
        (IpAddr::V4(_), IpAddr::V6(_)) => Ordering::Less,
        (IpAddr::V6(_), IpAddr::V4(_)) => Ordering::Greater,
    }
}

fn compare_natural(left: &str, right: &str) -> Ordering {
    let mut left_runs = NaturalRuns::new(left);
    let mut right_runs = NaturalRuns::new(right);

    loop {
        match (left_runs.next(), right_runs.next()) {
            (Some(left), Some(right)) => {
                let ordering = match (left.digits, right.digits) {
                    (true, true) => compare_digit_runs(left.text, right.text),
                    _ => left.text.cmp(right.text),
                };
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (None, None) => return Ordering::Equal,
        }
    }
}

fn compare_digit_runs(left: &str, right: &str) -> Ordering {
    let left_significant = left.trim_start_matches('0');
    let right_significant = right.trim_start_matches('0');
    let left_significant = if left_significant.is_empty() {
        "0"
    } else {
        left_significant
    };
    let right_significant = if right_significant.is_empty() {
        "0"
    } else {
        right_significant
    };

    left_significant
        .len()
        .cmp(&right_significant.len())
        .then_with(|| left_significant.cmp(right_significant))
        .then_with(|| left.len().cmp(&right.len()))
}

struct NaturalRun<'a> {
    text: &'a str,
    digits: bool,
}

struct NaturalRuns<'a> {
    source: &'a str,
    offset: usize,
}

impl<'a> NaturalRuns<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, offset: 0 }
    }
}

impl<'a> Iterator for NaturalRuns<'a> {
    type Item = NaturalRun<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let rest = self.source.get(self.offset..)?;
        let first = rest.chars().next()?;
        let digits = first.is_ascii_digit();
        let end = rest
            .char_indices()
            .skip(1)
            .find_map(|(index, ch)| (ch.is_ascii_digit() != digits).then_some(index))
            .unwrap_or(rest.len());
        self.offset += end;
        Some(NaturalRun {
            text: &rest[..end],
            digits,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::{cast_value, compare_natural, CastValue};
    use crate::predicate::ValueCast;
    use serde_json::json;

    #[test]
    fn natural_order_compares_numeric_runs_by_magnitude_and_zero_count() {
        assert_eq!(compare_natural("host9", "host10"), Ordering::Less);
        assert_eq!(compare_natural("host9", "host09"), Ordering::Less);
        assert_eq!(compare_natural("host009", "host09"), Ordering::Greater);
    }

    #[test]
    fn strict_casts_reject_partial_values() {
        assert!(cast_value(&json!("3tail"), ValueCast::Number).is_err());
        assert!(cast_value(&json!("yes"), ValueCast::Boolean).is_err());
        assert!(cast_value(&json!("192.0.2.999"), ValueCast::Ip).is_err());
    }

    #[test]
    fn ip_cast_keeps_address_families_distinct() {
        let ipv4 = cast_value(&json!("192.0.2.1"), ValueCast::Ip)
            .expect("valid cast")
            .expect("non-null");
        let mapped = cast_value(&json!("::ffff:192.0.2.1"), ValueCast::Ip)
            .expect("valid cast")
            .expect("non-null");
        assert!(matches!(ipv4, CastValue::Ip(std::net::IpAddr::V4(_))));
        assert!(matches!(mapped, CastValue::Ip(std::net::IpAddr::V6(_))));
    }
}
