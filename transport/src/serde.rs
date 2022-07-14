
use std::{borrow::Cow, ops::Deref};

use base64::display::Base64Display;
use serde::{Serializer, Deserializer, de::Visitor, Serialize, Deserialize};

#[derive(Clone, Debug)]
pub struct BytesOrB64<'a>(pub Cow<'a, [u8]>);

impl<'a, X: Into<Cow<'a, [u8]>>> From<X> for BytesOrB64<'a> {
    fn from(x: X) -> Self {
        BytesOrB64(x.into())
    }
}

impl<'a> Deref for BytesOrB64<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl<'a> Serialize for BytesOrB64<'a> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&Base64Display::with_config(&self.0, base64::STANDARD))
        } else {
            serde_bytes::serialize(&self.0, s)
        }
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for BytesOrB64<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = if d.is_human_readable() {
            Cow::Owned(d.deserialize_str(Base64Visitor)?)
        } else {
            serde_bytes::deserialize(d)?
        };
        Ok(BytesOrB64(r))
    }
}


struct Base64Visitor;
impl<'de> Visitor<'de> for Base64Visitor {
    type Value = Vec<u8>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a base64 string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_bytes(v.as_ref())
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_bytes(v.as_ref())
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        base64::decode(v).map_err(serde::de::Error::custom)
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_bytes(v.as_ref())
    }
}
