use std::fmt;
use std::str::FromStr;

use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, Value, ValueRef};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::primitives::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContentHash(String);

impl ContentHash {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut text = String::with_capacity(64);
        for byte in digest {
            use std::fmt::Write;
            write!(&mut text, "{byte:02x}").expect("writing to a String cannot fail");
        }
        Self(text)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ContentHash {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(Error::InvalidValue("sha256 hash", value.to_owned()));
        }
        Ok(Self(value.to_owned()))
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for ContentHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HashVisitor;

        impl<'de> Visitor<'de> for HashVisitor {
            type Value = ContentHash;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a 64 character lowercase sha256 hex string")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(HashVisitor)
    }
}

impl ToSql for ContentHash {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Text(self.0.clone())))
    }
}

impl FromSql for ContentHash {
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
    fn hashes_known_bytes() {
        assert_eq!(
            ContentHash::from_bytes(b"reinforce").to_string(),
            "5c17fb27fc5002b22e3158a8dbcc866b39a059174a2d8027a035180f18bbceb6"
        );
    }

    #[test]
    fn rejects_malformed_hash() {
        assert!("abc".parse::<ContentHash>().is_err());
        assert!("A".repeat(64).parse::<ContentHash>().is_err());
    }
}
