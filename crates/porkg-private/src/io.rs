use std::{
    future::Future,
    os::{
        fd::RawFd,
        unix::{
            net::UnixStream,
            prelude::{FromRawFd, OwnedFd},
        },
    },
};

use async_io::Async;
use bytes::{Buf, BufMut};
use uds::UnixStreamExt;

pub trait DomainSocket {
    fn send_all(&self, data: &mut impl Buf, fds: &[RawFd]) -> Result<(), std::io::Error>;
    fn recv_exact(
        &self,
        data: &mut impl BufMut,
        fds: &mut impl Extend<OwnedFd>,
    ) -> Result<(), std::io::Error>;
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
        let mut buffer = [0u8; 16384];
        let mut fd_buffer = [0i32; 1024];

        while data.has_remaining_mut() {
            let (buf_size, fds_size) =
                self.recv_fds(&mut buffer[..data.remaining_mut()], &mut fd_buffer)?;
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
        let mut buffer = [0u8; 16384];
        let mut fd_buffer = [0i32; 1024];

        while data.has_remaining_mut() {
            let (buf_size, fds_size) = self
                .read_with(|s| s.recv_fds(&mut buffer[..data.remaining_mut()], &mut fd_buffer[..]))
                .await?;
            fds.extend(
                fd_buffer[..fds_size]
                    .iter()
                    .map(|v| unsafe { OwnedFd::from_raw_fd(*v) }),
            );
            data.put(&buffer[..buf_size])
        }
        Ok(())
    }
}
