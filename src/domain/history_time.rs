use chrono::{DateTime, Duration, Months, NaiveDateTime, Utc};
use hubuum_client::HubuumDateTime;

use crate::errors::AppError;

/// An exact as-of instant, or the half-open UTC interval described by a date prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryTime {
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
}

impl HistoryTime {
    pub(crate) fn parse(value: &str) -> Result<Self, AppError> {
        Self::parse_at(value, Utc::now())
    }

    fn parse_at(value: &str, now: DateTime<Utc>) -> Result<Self, AppError> {
        if let Ok(instant) = DateTime::parse_from_rfc3339(value) {
            return Ok(Self {
                start: instant.with_timezone(&Utc),
                end: None,
            });
        }
        if value == "now" {
            return Ok(Self {
                start: now,
                end: None,
            });
        }
        if let Some(start) = relative_time(value, now) {
            return Ok(Self { start, end: None });
        }
        let suffix = match value.len() {
            4 => "-01-01T00:00:00",
            7 => "-01T00:00:00",
            10 => "T00:00:00",
            13 => ":00:00",
            16 => ":00",
            19 => "",
            _ => return Err(invalid_time()),
        };
        // Check the shape as well as the calendar; chrono accepts unpadded components.
        let template = "0000-00-00T00:00:00";
        if !value
            .bytes()
            .zip(template.bytes())
            .all(|(value, expected)| {
                if expected == b'0' {
                    value.is_ascii_digit()
                } else {
                    value == expected
                }
            })
        {
            return Err(invalid_time());
        }
        let start = NaiveDateTime::parse_from_str(&format!("{value}{suffix}"), "%Y-%m-%dT%H:%M:%S")
            .map_err(|_| invalid_time())?
            .and_utc();
        let end = match value.len() {
            4 => start.checked_add_months(Months::new(12)),
            7 => start.checked_add_months(Months::new(1)),
            10 => start.checked_add_signed(Duration::days(1)),
            13 => start.checked_add_signed(Duration::hours(1)),
            16 => start.checked_add_signed(Duration::minutes(1)),
            _ => start.checked_add_signed(Duration::seconds(1)),
        }
        .ok_or_else(invalid_time)?;
        Ok(Self {
            start,
            end: Some(end),
        })
    }

    pub(crate) fn start(&self) -> HubuumDateTime {
        HubuumDateTime(self.start)
    }

    pub(crate) fn end(&self) -> Option<HubuumDateTime> {
        self.end.map(HubuumDateTime)
    }
}

fn invalid_time() -> AppError {
    AppError::InvalidOption(
        "Expected an RFC3339 timestamp, a UTC date/time prefix (YYYY[-MM[-DD[THH[:MM[:SS]]]]]), or a time ago such as 1mo1w4d3h44m1s (optional leading '-'; mo = calendar months, m = minutes)".into(),
    )
}

fn relative_time(value: &str, mut now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let mut rest = value.strip_prefix('-').unwrap_or(value);
    let mut last_unit = None;
    while !rest.is_empty() {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let amount: u32 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let (unit, rank, seconds) = [
            ("mo", 0, 0),
            ("w", 1, 604_800),
            ("d", 2, 86_400),
            ("h", 3, 3_600),
            ("m", 4, 60),
            ("s", 5, 1),
        ]
        .into_iter()
        .find(|(unit, _, _)| rest.starts_with(unit))?;
        if last_unit.is_some_and(|last| rank <= last) {
            return None;
        }
        last_unit = Some(rank);
        rest = &rest[unit.len()..];
        now = if unit == "mo" {
            now.checked_sub_months(Months::new(amount))?
        } else {
            now.checked_sub_signed(Duration::seconds(i64::from(amount) * seconds))?
        };
    }
    last_unit.map(|_| now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn prefixes_cover_exact_calendar_intervals() {
        for (value, start, end) in [
            ("2026", "2026-01-01T00:00:00Z", "2027-01-01T00:00:00Z"),
            ("2024-02", "2024-02-01T00:00:00Z", "2024-03-01T00:00:00Z"),
            ("2026-12-31", "2026-12-31T00:00:00Z", "2027-01-01T00:00:00Z"),
            (
                "2026-01-01T15",
                "2026-01-01T15:00:00Z",
                "2026-01-01T16:00:00Z",
            ),
            (
                "2026-01-01T15:59",
                "2026-01-01T15:59:00Z",
                "2026-01-01T16:00:00Z",
            ),
            (
                "2026-01-01T15:00:00",
                "2026-01-01T15:00:00Z",
                "2026-01-01T15:00:01Z",
            ),
        ] {
            let parsed = HistoryTime::parse(value).unwrap();
            assert_eq!(parsed.start, instant(start), "{value}");
            assert_eq!(parsed.end, Some(instant(end)), "{value}");
        }
    }

    #[test]
    fn exact_timestamps_keep_offsets_and_subseconds() {
        let parsed = HistoryTime::parse("2026-01-01T15:00:00.123456+02:00").unwrap();
        assert_eq!(parsed.start, instant("2026-01-01T13:00:00.123456Z"));
        assert_eq!(parsed.end, None);
    }

    #[test]
    fn relative_parts_are_optional_and_months_use_the_calendar() {
        let now = instant("2026-03-31T12:00:00Z");
        for value in ["1mo1w4d3h44m1s", "-1mo1w4d3h44m1s"] {
            assert_eq!(
                HistoryTime::parse_at(value, now).unwrap().start,
                instant("2026-02-17T08:15:59Z")
            );
        }
        for (value, seconds) in [
            ("1w", 604800),
            ("1d", 86400),
            ("3h", 10800),
            ("44m", 2640),
            ("1s", 1),
            ("0s", 0),
        ] {
            assert_eq!(
                HistoryTime::parse_at(value, now).unwrap().start,
                now - Duration::seconds(seconds)
            );
        }
        assert_eq!(HistoryTime::parse_at("now", now).unwrap().start, now);
    }

    #[test]
    fn invalid_dates_durations_and_overflow_are_rejected() {
        for value in [
            "",
            "-",
            "2026-02-29",
            "2026-13",
            "2026-1",
            "2026-01-01T24",
            "tomorrow",
            "1m1m",
            "1s1h",
            "+1h",
            "1.5h",
            "1x",
            "99999999999999999mo",
            "4294967295w",
        ] {
            assert!(HistoryTime::parse(value).is_err(), "{value}");
        }
    }
}
