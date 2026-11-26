use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, Value, ValueRef};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;

use crate::primitives::Error;

/// Fixed-width UTC representation so text ordering matches chronological ordering.
const STORAGE_FORMAT: &[time::format_description::FormatItem<'_>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:6]Z");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(OffsetDateTime);

fn truncated_to_micros(value: OffsetDateTime) -> OffsetDateTime {
    value
        .replace_nanosecond(value.nanosecond() / 1_000 * 1_000)
        .expect("nanoseconds derived from an existing timestamp fit in a second")
}

/// RFC3339 allows arbitrarily long fractions; `time` parses up to nanoseconds.
fn fraction_truncated_to_nanoseconds(value: &str) -> std::borrow::Cow<'_, str> {
    let Some(time_start) = value.find('T').or_else(|| value.find('t')) else {
        return std::borrow::Cow::Borrowed(value);
    };
    let Some(dot) = value[time_start..].find('.') else {
        return std::borrow::Cow::Borrowed(value);
    };
    let digits_start = time_start + dot + 1;
    let digits_end = value[digits_start..]
        .find(|character: char| !character.is_ascii_digit())
        .map_or(value.len(), |offset| digits_start + offset);
    if digits_end - digits_start <= 9 {
        return std::borrow::Cow::Borrowed(value);
    }

    let mut normalized = String::with_capacity(value.len());
    normalized.push_str(&value[..digits_start + 9]);
    normalized.push_str(&value[digits_end..]);
    std::borrow::Cow::Owned(normalized)
}

impl Timestamp {
    pub fn now() -> Self {
        Self::from_offset(OffsetDateTime::now_utc())
    }

    pub fn from_offset(value: OffsetDateTime) -> Self {
        Self(truncated_to_micros(value.to_offset(time::UtcOffset::UTC)))
    }

    pub fn unix_epoch() -> Self {
        Self(OffsetDateTime::UNIX_EPOCH)
    }

    pub fn plus_days(&self, days: f64) -> Self {
        let seconds = (days * 86_400.0).round() as i64;
        Self(self.0 + time::Duration::seconds(seconds))
    }

    pub fn days_since(&self, earlier: &Self) -> f64 {
        (self.0 - earlier.0).as_seconds_f64() / 86_400.0
    }

    pub fn as_datetime(&self) -> OffsetDateTime {
        self.0
    }

    pub fn as_str(&self) -> String {
        self.0
            .format(STORAGE_FORMAT)
            .expect("an internally built timestamp is always representable")
    }
}

impl FromStr for Timestamp {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = fraction_truncated_to_nanoseconds(value);
        let parsed = OffsetDateTime::parse(&normalized, &Rfc3339)
            .map_err(|_| Error::InvalidValue("RFC3339 timestamp", value.to_owned()))?;
        Ok(Self::from_offset(parsed))
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_str())
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TimestampVisitor;

        impl<'de> Visitor<'de> for TimestampVisitor {
            type Value = Timestamp;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an RFC3339 timestamp")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(TimestampVisitor)
    }
}

impl ToSql for Timestamp {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Text(self.as_str())))
    }
}

impl FromSql for Timestamp {
    fn column_result(value: ValueRef<'_>) -> Result<Self, FromSqlError> {
        let text = value.as_str()?;
        text.parse()
            .map_err(|error| FromSqlError::Other(Box::new(error)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utc_rfc3339() {
        let parsed: Timestamp = "2026-11-26T07:19:13Z".parse().unwrap();
        let roundtrip: Timestamp = parsed.as_str().parse().unwrap();
        assert_eq!(parsed, roundtrip);
        assert!(parsed.as_str().contains("2026-11-26"));
    }

    #[test]
    fn formats_fixed_width_utc_and_normalizes_offsets() {
        let parsed: Timestamp = "2026-11-26T15:19:13.123456789+08:00".parse().unwrap();
        assert_eq!(parsed.as_str(), "2026-11-26T07:19:13.123456Z");
        let roundtrip: Timestamp = parsed.as_str().parse().unwrap();
        assert_eq!(roundtrip, parsed);
        assert_eq!(roundtrip.as_str(), parsed.as_str());

        let whole_seconds: Timestamp = "2026-11-26T07:19:13Z".parse().unwrap();
        assert_eq!(whole_seconds.as_str(), "2026-11-26T07:19:13.000000Z");

        let long_fraction: Timestamp = "2026-11-26T07:19:13.123456789999Z".parse().unwrap();
        assert_eq!(long_fraction.as_str(), "2026-11-26T07:19:13.123456Z");
    }

    #[test]
    fn ordering_follows_instant_not_text() {
        let whole_seconds: Timestamp = "2026-11-26T07:19:13Z".parse().unwrap();
        let fractional: Timestamp = "2026-11-26T07:19:13.5Z".parse().unwrap();
        let next_second: Timestamp = "2026-11-26T07:19:14Z".parse().unwrap();

        assert!(whole_seconds < fractional);
        assert!(fractional < next_second);
        assert!(whole_seconds.as_str() < fractional.as_str());
    }
}
