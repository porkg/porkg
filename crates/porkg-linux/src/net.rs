use std::os::{fd::RawFd, unix::net::UnixStream};

pub struct DomainSocket {
    socket: UnixStream,
}

impl DomainSocket {
    pub fn write(&mut self, data: &[u8], fds: &[RawFd]) {}
}
