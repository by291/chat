//! Sequence values cross JSON as decimal strings to retain all 64 bits in JavaScript.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, sqlx::Type, ToSchema)]
#[sqlx(transparent)]
#[schema(value_type=String,pattern="^(0|[1-9][0-9]*)$")]
pub struct Seq(pub i64);

impl Serialize for Seq {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for Seq {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        if raw.is_empty()
            || (raw.len() > 1 && raw.starts_with('0'))
            || !raw.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(serde::de::Error::custom("invalid sequence"));
        }
        let sequence = raw.parse::<i64>().map_err(serde::de::Error::custom)?;
        Ok(Self(sequence))
    }
}
