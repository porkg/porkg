use std::{
    future::Future,
    mem::size_of,
    os::{
        fd::RawFd,
        unix::{
            net::UnixStream,
            prelude::{FromRawFd, OwnedFd},
        },
    },
};

use async_io::Async;
use bytes::{buf::Limit, Buf, BufMut, BytesMut};
use thiserror::Error;
use uds::UnixStreamExt;

use crate::{mem::get_buffer, ser};

const READ_BUFFER_SIZE: usize = 8192;
const FD_BUFFER_SIZE: usize = 128;
const HEADER_SIZE: usize = size_of::<usize>();

pub trait LimitExt {
    fn reserve_and_limit(&mut self, len: usize) -> Limit<&mut Self>;
}

impl LimitExt for BytesMut {
    fn reserve_and_limit(&mut self, len: usize) -> Limit<&mut Self> {
        self.reserve(len);
        <&mut BytesMut>::limit(self, len)
    }
}

#[derive(Debug, Error)]
pub enum SocketMessageError {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Serialize(#[from] ser::Error),
}

pub trait DomainSocket {
    fn send_all(&self, data: &mut impl Buf, fds: &[RawFd]) -> Result<(), std::io::Error>;
    fn recv_exact(
        &self,
        data: &mut impl BufMut,
        fds: &mut impl Extend<OwnedFd>,
    ) -> Result<(), std::io::Error>;

    fn send_message<T: crate::ser::Serialize>(
        &self,
        message: &T,
        fds: &[RawFd],
    ) -> Result<(), SocketMessageError> {
        let mut buf = get_buffer();

        buf.put_slice(&[0u8; HEADER_SIZE]);
        ser::serialize(message, buf.as_mut())?;

        let len = buf.len() - HEADER_SIZE;
        buf[..HEADER_SIZE].copy_from_slice(&len.to_ne_bytes());

        self.send_all(buf.as_mut(), fds)?;

        Ok(())
    }

    fn recv_message<T: crate::ser::Deserialize>(
        &self,
        fds: &mut impl Extend<OwnedFd>,
    ) -> Result<T, SocketMessageError> {
        let mut buf = get_buffer();

        self.recv_exact(&mut buf.reserve_and_limit(HEADER_SIZE), fds)?;
        let len = usize::from_ne_bytes(buf[..HEADER_SIZE].try_into().unwrap());

        buf.clear();
        self.recv_exact(&mut buf.reserve_and_limit(len), fds)?;

        let result = ser::deserialize(buf.as_mut())?;
        Ok(result)
    }
}

impl DomainSocket for UnixStream {
    fn send_all(&self, data: &mut impl Buf, mut fds: &[RawFd]) -> Result<(), std::io::Error> {
        while data.has_remaining() {
            let remaining = data.chunk();
            let size = self.send_fds(remaining, fds)?;
            data.advance(size);
            fds = &[];
        }
        Ok(())
    }

    fn recv_exact(
        &self,
        data: &mut impl BufMut,
        fds: &mut impl Extend<OwnedFd>,
    ) -> Result<(), std::io::Error> {
        let mut buffer = [0u8; READ_BUFFER_SIZE];
        let mut fd_buffer = [0i32; FD_BUFFER_SIZE];

        while data.has_remaining_mut() {
            let to_read = buffer.len().min(data.remaining_mut());
            let (buf_size, fds_size) = self.recv_fds(&mut buffer[..to_read], &mut fd_buffer)?;
            fds.extend(
                fd_buffer[..fds_size]
                    .iter()
                    .map(|v| unsafe { OwnedFd::from_raw_fd(*v) }),
            );
            data.put(&buffer[..buf_size]);
        }
        Ok(())
    }
}

pub trait DomainSocketAsync {
    fn send_all(
        &self,
        data: &mut (impl Buf + Send),
        fds: &[RawFd],
    ) -> impl Send + Future<Output = Result<(), std::io::Error>>;

    fn recv_exact(
        &self,
        data: &mut (impl BufMut + Send + Sync),
        fds: &mut (impl Extend<OwnedFd> + Send),
    ) -> impl Send + Future<Output = Result<(), std::io::Error>>;
}

pub trait DomainSocketAsyncExt {
    fn send_message<T: crate::ser::Serialize + Send + Sync>(
        &self,
        message: &T,
        fds: &[RawFd],
    ) -> impl Send + Future<Output = Result<(), SocketMessageError>>;

    fn recv_message<T: crate::ser::Deserialize + Send + Sync>(
        &self,
        fds: &mut (impl Extend<OwnedFd> + Send),
    ) -> impl Send + Future<Output = Result<T, SocketMessageError>>;
}

impl<S: DomainSocketAsync + Send + Sync> DomainSocketAsyncExt for S {
    async fn send_message<T: crate::ser::Serialize + Send + Sync>(
        &self,
        message: &T,
        fds: &[RawFd],
    ) -> Result<(), SocketMessageError> {
        let mut buf = get_buffer();

        buf.put_slice(&[0u8; HEADER_SIZE]);
        ser::serialize(message, buf.as_mut())?;

        let len = buf.len() - HEADER_SIZE;
        buf[..HEADER_SIZE].copy_from_slice(&len.to_ne_bytes());

        self.send_all(buf.as_mut(), fds).await?;

        Ok(())
    }

    async fn recv_message<T: crate::ser::Deserialize>(
        &self,
        fds: &mut (impl Extend<OwnedFd> + Send),
    ) -> Result<T, SocketMessageError> {
        let mut buf = get_buffer();

        self.recv_exact(&mut buf.reserve_and_limit(HEADER_SIZE), fds)
            .await?;
        let len = usize::from_ne_bytes(buf[..HEADER_SIZE].try_into().unwrap());

        buf.clear();
        self.recv_exact(&mut buf.reserve_and_limit(len), fds)
            .await?;

        let result = ser::deserialize(buf.as_mut())?;
        Ok(result)
    }
}

impl DomainSocketAsync for Async<UnixStream> {
    async fn send_all(&self, data: &mut impl Buf, mut fds: &[RawFd]) -> Result<(), std::io::Error> {
        while data.has_remaining() {
            let remaining = data.chunk();
            let size = self.write_with(|s| s.send_fds(remaining, fds)).await?;
            data.advance(size);
            fds = &[];
        }
        Ok(())
    }

    async fn recv_exact(
        &self,
        data: &mut impl BufMut,
        fds: &mut impl Extend<OwnedFd>,
    ) -> Result<(), std::io::Error> {
        let mut buffer = [0u8; READ_BUFFER_SIZE];
        let mut fd_buffer = [0i32; FD_BUFFER_SIZE];

        while data.has_remaining_mut() {
            let to_read = buffer.len().min(data.remaining_mut());
            let (buf_size, fds_size) = self
                .read_with(|s| s.recv_fds(&mut buffer[..to_read], &mut fd_buffer[..]))
                .await?;
            fds.extend(
                fd_buffer[..fds_size]
                    .iter()
                    .map(|v| unsafe { OwnedFd::from_raw_fd(*v) }),
            );
            data.put(&buffer[..buf_size]);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::os::{fd::AsRawFd as _, unix::net::UnixStream};

    use async_io::Async;
    use pretty_assertions::assert_eq;
    use serde::{Deserialize, Serialize};

    use crate::io::DomainSocketAsyncExt as _;

    use super::DomainSocket;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct SomeMessage {
        value: i32,
    }

    #[test]
    pub fn send_recv_message() {
        let (a, b) = UnixStream::pair().unwrap();
        let msg = SomeMessage { value: 42 };

        a.send_message(&msg, &[]).unwrap();

        let mut fds = Vec::new();
        let r: SomeMessage = b.recv_message(&mut fds).unwrap();

        assert_eq!(msg, r);
        assert!(fds.is_empty());
    }

    #[test]
    pub fn send_recv_message_fds() {
        let (c, d) = UnixStream::pair().unwrap();
        let (a, b) = UnixStream::pair().unwrap();

        let msg = SomeMessage { value: 42 };

        a.send_message(&msg, &[c.as_raw_fd()]).unwrap();
        drop(c);

        let mut fds = Vec::new();
        let r: SomeMessage = b.recv_message(&mut fds).unwrap();

        assert_eq!(msg, r);

        let c = fds.into_iter().next().unwrap();
        let c: UnixStream = c.into();

        c.send_message(&msg, &[]).unwrap();

        let mut fds = Vec::new();
        let r: SomeMessage = d.recv_message(&mut fds).unwrap();

        assert_eq!(msg, r);
    }

    #[test]
    pub fn send_recv_message_fds_many() {
        let (c, d) = UnixStream::pair().unwrap();
        let (a, b) = UnixStream::pair().unwrap();

        let msg = SomeMessage { value: 42 };

        a.send_message(&msg, &[c.as_raw_fd()]).unwrap();
        a.send_message(&msg, &[c.as_raw_fd()]).unwrap();
        drop(c);

        let mut fds = Vec::new();
        let r: SomeMessage = b.recv_message(&mut fds).unwrap();

        assert_eq!(msg, r);
        assert_eq!(1, fds.len());

        let c = fds.into_iter().next().unwrap();
        let c: UnixStream = c.into();

        c.send_message(&msg, &[]).unwrap();

        let mut fds = Vec::new();
        let r: SomeMessage = d.recv_message(&mut fds).unwrap();

        assert_eq!(msg, r);
    }

    #[test]
    pub fn async_send_recv_message() {
        async_io::block_on(async {
            let (a, b) = UnixStream::pair().unwrap();
            let a = Async::new(a).unwrap();
            let b = Async::new(b).unwrap();
            let msg = SomeMessage { value: 42 };

            a.send_message(&msg, &[]).await.unwrap();

            let mut fds = Vec::new();
            let r: SomeMessage = b.recv_message(&mut fds).await.unwrap();

            assert_eq!(msg, r);
            assert!(fds.is_empty());
        });
    }

    #[test]
    pub fn async_send_recv_message_fds() {
        async_io::block_on(async {
            let (c, d) = UnixStream::pair().unwrap();
            let (a, b) = UnixStream::pair().unwrap();
            let a = Async::new(a).unwrap();
            let b = Async::new(b).unwrap();
            let c = Async::new(c).unwrap();
            let d = Async::new(d).unwrap();

            let msg = SomeMessage { value: 42 };

            a.send_message(&msg, &[c.as_raw_fd()]).await.unwrap();
            drop(c);

            let mut fds = Vec::new();
            let r: SomeMessage = b.recv_message(&mut fds).await.unwrap();

            assert_eq!(msg, r);

            let c = fds.into_iter().next().unwrap();
            let c: UnixStream = c.into();
            let c = Async::new(c).unwrap();

            c.send_message(&msg, &[]).await.unwrap();

            let mut fds = Vec::new();
            let r: SomeMessage = d.recv_message(&mut fds).await.unwrap();

            assert_eq!(msg, r);
        });
    }

    #[test]
    pub fn async_send_recv_message_fds_many() {
        async_io::block_on(async {
            let (c, d) = UnixStream::pair().unwrap();
            let (a, b) = UnixStream::pair().unwrap();
            let a = Async::new(a).unwrap();
            let b = Async::new(b).unwrap();
            let c = Async::new(c).unwrap();
            let d = Async::new(d).unwrap();

            let msg = SomeMessage { value: 42 };

            a.send_message(&msg, &[c.as_raw_fd()]).await.unwrap();
            a.send_message(&msg, &[c.as_raw_fd()]).await.unwrap();
            drop(c);

            let mut fds = Vec::new();
            let r: SomeMessage = b.recv_message(&mut fds).await.unwrap();

            assert_eq!(msg, r);
            assert_eq!(1, fds.len());

            let c = fds.into_iter().next().unwrap();
            let c: UnixStream = c.into();
            let c = Async::new(c).unwrap();

            c.send_message(&msg, &[]).await.unwrap();

            let mut fds = Vec::new();
            let r: SomeMessage = d.recv_message(&mut fds).await.unwrap();

            assert_eq!(msg, r);
        });
    }
}
