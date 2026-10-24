use chrono::{DateTime, Duration, Utc};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::PipelineError;

pub(crate) fn evaluation_clock(time: SystemTime) -> Result<DateTime<Utc>, PipelineError> {
    let invalid =
        || PipelineError::Pipe("Evaluation time is outside the supported datetime range".into());
    let (seconds, nanos) = match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => (
            i64::try_from(duration.as_secs()).map_err(|_| invalid())?,
            duration.subsec_nanos(),
        ),
        Err(error) => {
            let duration = error.duration();
            let seconds = i64::try_from(duration.as_secs()).map_err(|_| invalid())?;
            if duration.subsec_nanos() == 0 {
                (-seconds, 0)
            } else {
                (
                    seconds
                        .checked_neg()
                        .and_then(|n| n.checked_sub(1))
                        .ok_or_else(invalid)?,
                    1_000_000_000 - duration.subsec_nanos(),
                )
            }
        }
    };
    DateTime::from_timestamp(seconds, nanos).ok_or_else(invalid)
}

/// A validated duration before the evaluation's shared clock sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelativeTime {
    seconds_ago: i64,
}

impl RelativeTime {
    pub fn parse(source: &str) -> Result<Self, PipelineError> {
        if source == "now" {
            return Ok(Self { seconds_ago: 0 });
        }
        let invalid = || {
            PipelineError::Parse("Relative time must be now or now-Ns/m/h/d with a nonnegative integer duration in range".into())
        };
        let duration = source.strip_prefix("now-").ok_or_else(invalid)?;
        let (amount, unit) = duration.split_at(
            duration
                .len()
                .checked_sub(1)
                .filter(|&i| duration.is_char_boundary(i))
                .ok_or_else(invalid)?,
        );
        let multiplier = match unit {
            "s" => 1,
            "m" => 60,
            "h" => 3600,
            "d" => 86400,
            _ => return Err(invalid()),
        };
        if amount.is_empty() || !amount.bytes().all(|ch| ch.is_ascii_digit()) {
            return Err(invalid());
        }
        let seconds_ago = amount
            .parse::<i64>()
            .ok()
            .and_then(|n| n.checked_mul(multiplier))
            .filter(|&n| Duration::try_seconds(n).is_some())
            .ok_or_else(invalid)?;
        Ok(Self { seconds_ago })
    }

    pub fn seconds_ago(self) -> i64 {
        self.seconds_ago
    }

    pub(crate) fn resolve(self, now: DateTime<Utc>) -> Result<String, PipelineError> {
        now.checked_sub_signed(Duration::seconds(self.seconds_ago))
            .map(|time| time.to_rfc3339())
            .ok_or_else(|| {
                PipelineError::Pipe("Relative time is outside the supported datetime range".into())
            })
    }
}
