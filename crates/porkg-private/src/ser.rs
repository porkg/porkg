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
