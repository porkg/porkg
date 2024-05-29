//! Declares the internal serialization format by re-exporting it.

use std::mem::size_of;

pub use bincode::Error;
use bytes::{Buf, BufMut, BytesMut};
pub use serde::{de::DeserializeOwned as Deserialize, Serialize};

pub const HEADER_SIZE: usize = size_of::<usize>();

pub fn serialize<T: Serialize + ?Sized>(data: &T, buf: &mut impl BufMut) -> Result<(), Error> {
    let writer = buf.writer();
    bincode::serialize_into(writer, data)
}

pub fn serialize_with_prefix<T: Serialize + ?Sized>(
    data: &T,
    mut buf: impl AsMut<BytesMut>,
) -> Result<(), Error> {
    let buf = buf.as_mut();
    let pos = buf.len();

    buf.put_slice(&[0u8; HEADER_SIZE]);
    serialize(data, buf)?;

    let len = buf.len() - HEADER_SIZE - pos;
    buf[pos..(pos + HEADER_SIZE)].copy_from_slice(&len.to_ne_bytes());

    Ok(())
}

pub fn deserialize<T: Deserialize + ?Sized>(buf: &mut impl Buf) -> Result<T, Error> {
    let reader = buf.reader();
    bincode::deserialize_from(reader)
}
