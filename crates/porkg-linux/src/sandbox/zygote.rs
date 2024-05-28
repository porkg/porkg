use std::{
    marker::PhantomData,
    os::{fd::OwnedFd, unix::net::UnixStream},
};

use async_io::Async;
use porkg_private::{io::DomainSocketAsync as _, os::proc::ChildProcess, sandbox::SandboxTask};
use thiserror::Error;

use crate::{
    clone::{CloneError, CloneFlags, CloneSyscall},
    private::Syscall,
};

#[derive(Debug, Error)]
enum CreateZygoteErrorKind {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Clone(#[from] CloneError),
}

#[derive(Debug, Error)]
#[error("failed to start the zygote: {source}")]
pub struct CreateZygoteError {
    #[source]
    source: CreateZygoteErrorKind,
}

#[derive(Debug, Error)]
enum ConnectZygoteErrorKind {
    #[error(transparent)]
    IO(#[from] std::io::Error),
}

#[derive(Debug, Error)]
#[error("failed to connect to the zygote: {source}")]
pub struct ConnectZygoteError {
    #[source]
    source: ConnectZygoteErrorKind,
}

pub struct Zygote<S: CloneSyscall = Syscall> {
    stream: Async<UnixStream>,
    proc: ChildProcess,
    _p: PhantomData<S>,
}

impl<S: CloneSyscall> Zygote<S> {
    #[tracing::instrument]
    pub fn create_zygote() -> Result<(UnixStream, ChildProcess), CreateZygoteError> {
        let (parent, child) = UnixStream::pair()
            .inspect(|_| tracing::trace!("created socket pair for zygote communication"))
            .inspect_err(|error| {
                tracing::error!(
                    ?error,
                    "failed to create socket pair for zygote communication"
                )
            })
            .map_err(|source| CreateZygoteError {
                source: source.into(),
            })?;

        let cb = Box::new(move || match child.try_clone() {
            Ok(child) => Ok(()),
            Err(e) => Err(anyhow::anyhow!("failed to clone child socket: {0}", e)),
        });

        let zygote: ChildProcess = S::clone(cb, CloneFlags::empty())
            .inspect(|pid| tracing::trace!(?pid, "started zygote process"))
            .inspect_err(|error| tracing::error!(?error, "failed to start zygote process"))
            .map_err(|source| CreateZygoteError {
                source: source.into(),
            })?
            .into();

        Ok((parent, zygote))
    }

    pub fn connect(stream: UnixStream, proc: ChildProcess) -> Result<Self, ConnectZygoteError> {
        let stream = Async::new(stream)
            .inspect_err(|error| tracing::error!(?error, "failed to make socket async"))
            .map_err(|source| ConnectZygoteError {
                source: source.into(),
            })?;
        Ok(Zygote {
            stream,
            proc,
            _p: PhantomData,
        })
    }

    pub async fn spawn_async<T: SandboxTask>(&self, task: T) -> Result<(), ConnectZygoteError> {
        let mut buf = Vec::<u8>::new();
        let mut fds = Vec::<OwnedFd>::new();

        self.stream
            .recv_exact(&mut buf, &mut fds)
            .await
            .map_err(|source| ConnectZygoteError {
                source: source.into(),
            })?;

        Ok(())
    }
}
