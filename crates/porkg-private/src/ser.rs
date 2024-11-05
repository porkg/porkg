//! Declares the internal serialization format by re-exporting it.

use std::mem::size_of;

pub use bincode::Error;
use bytes::{Buf, BufMut};
pub use serde::{de::DeserializeOwned as Deserialize, Serialize};

pub const HEADER_SIZE: usize = size_of::<usize>();

pub fn serialize<T: Serialize + ?Sized>(data: &T, buf: &mut impl BufMut) -> Result<(), Error> {
    let writer = buf.writer();
    bincode::serialize_into(writer, data)
}

pub fn deserialize<T: Deserialize + ?Sized>(buf: &mut impl Buf) -> Result<T, Error> {
    let reader = buf.reader();
    bincode::deserialize_from(reader)
}

pub mod fromstr {
    use serde::{de, Deserialize};
    use std::str::FromStr;

    pub fn deserialize<'de, D, R: FromStr>(deserializer: D) -> Result<R, D::Error>
    where
        D: de::Deserializer<'de>,
        R::Err: std::fmt::Display,
    {
        let s = <&str>::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

pub mod pathbuf {
    use serde::{de, ser};
    use std::{fmt, path::PathBuf};

    struct SerializedPathBuf;

    impl<'de> de::Visitor<'de> for SerializedPathBuf {
        type Value = PathBuf;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string containing a path")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            v.parse().map_err(E::custom)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            // Environment variables in config always appear as sequences
            if let Some(v) = seq.next_element()? {
                if seq.next_element::<Self::Value>().ok().flatten().is_none() {
                    return Ok(v);
                }
            }
            Err(de::Error::invalid_length(0, &self))
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(SerializedPathBuf)
    }

    #[allow(clippy::ptr_arg)] // Required by contract
    pub fn serialize<S>(val: &PathBuf, s: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        s.serialize_str(
            val.to_str()
                .ok_or_else(|| ser::Error::custom("expected utf8 path"))?,
        )
    }
}
