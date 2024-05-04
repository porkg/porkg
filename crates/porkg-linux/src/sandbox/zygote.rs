use std::{
    marker::PhantomData,
    os::{fd::OwnedFd, unix::net::UnixStream},
};

use async_io::Async;
use nix::errno::Errno;
use porkg_private::{io::DomainSocketAsync as _, os::proc::ChildProcess, sandbox::SandboxTask};

use crate::{
    clone::{CloneFlags, CloneSyscall},
    private::Syscall,
};

#[derive(Debug, Clone, Default)]
pub struct CreateZygoteError;

impl crate::private::ErrorKind for CreateZygoteError {
    fn fmt(&self, err: Errno, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if err != nix::Error::UnknownErrno {
            write!(f, "failed to start the zygote: {0}", err)
        } else {
            write!(f, "failed to start the zygote")
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConnectZygoteError;

impl crate::private::ErrorKind for ConnectZygoteError {
    fn fmt(&self, err: Errno, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if err != nix::Error::UnknownErrno {
            write!(f, "failed to connect to the zygote: {0}", err)
        } else {
            write!(f, "failed to connect to the zygote")
        }
    }
}

pub struct Zygote<S: CloneSyscall = Syscall> {
    stream: Async<UnixStream>,
    proc: ChildProcess,
    _p: PhantomData<S>,
}

impl<S: CloneSyscall> Zygote<S> {
    #[tracing::instrument]
    pub fn create_zygote() -> crate::Result<(UnixStream, ChildProcess), CreateZygoteError> {
        let (parent, child) = UnixStream::pair()
            .inspect(|_| tracing::trace!("created socket pair for zygote communication"))
            .inspect_err(|error| {
                tracing::error!(
                    ?error,
                    "failed to create socket pair for zygote communication"
                )
            })
            .map_err(crate::Error::from_any)?;

        let cb = Box::new(move || match child.try_clone() {
            Ok(child) => Ok(()),
            Err(e) => Err(anyhow::anyhow!("failed to clone child socket: {0}", e)),
        });

        let zygote: ChildProcess = S::clone(cb, CloneFlags::empty())
            .inspect(|pid| tracing::trace!(?pid, "started zygote process"))
            .inspect_err(|error| tracing::error!(?error, "failed to start zygote process"))
            .map_err(crate::Error::from_any)?
            .into();

        Ok((parent, zygote))
    }

    pub fn connect(
        stream: UnixStream,
        proc: ChildProcess,
    ) -> crate::Result<Self, ConnectZygoteError> {
        let stream = Async::new(stream)
            .inspect_err(|error| tracing::error!(?error, "failed to make socket async"))
            .map_err(crate::Error::from_any)?;
        Ok(Zygote {
            stream,
            proc,
            _p: PhantomData,
        })
    }

    pub async fn spawn_async<T: SandboxTask>(
        &self,
        task: T,
    ) -> crate::Result<(), ConnectZygoteError> {
        let mut buf = Vec::<u8>::new();
        let mut fds = Vec::<OwnedFd>::new();

        self.stream
            .recv_exact(&mut buf, &mut fds)
            .await
            .map_err(crate::Error::from_any)?;

        Ok(())
    }
}
