use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, Value, ValueRef};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::primitives::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp(OffsetDateTime);

impl Timestamp {
    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    pub fn from_offset(value: OffsetDateTime) -> Self {
        Self(value.to_offset(time::UtcOffset::UTC))
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
            .format(&Rfc3339)
            .expect("an internally built timestamp is always RFC3339")
    }
}

impl FromStr for Timestamp {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parsed = OffsetDateTime::parse(value, &Rfc3339)
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
}
