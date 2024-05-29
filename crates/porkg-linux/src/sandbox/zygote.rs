use std::{
    marker::PhantomData,
    mem::size_of,
    os::{fd::OwnedFd, unix::net::UnixStream},
};

use async_io::Async;
use bytes::BufMut as _;
use porkg_private::{
    io::{DomainSocketAsync as _, LimitExt as _},
    mem::BUFFER_POOL,
    os::proc::ChildProcess,
    sandbox::SandboxTask,
    ser::{serialize, serialize_with_prefix},
};
use thiserror::Error;

use crate::{
    clone::{CloneError, CloneFlags, CloneSyscall},
    private::Syscall,
};

const USIZE_SIZE: usize = size_of::<usize>();

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

#[derive(Debug, Error)]
enum SpawnZygoteErrorKind {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Serialization(#[from] porkg_private::ser::Error),
}

#[derive(Debug, Error)]
#[error("failed to spawn a process from the zygote: {source}")]
pub struct SpawnZygoteError {
    #[source]
    #[from]
    source: SpawnZygoteErrorKind,
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

    pub async fn spawn_async<T: SandboxTask>(&self, task: T) -> Result<(), SpawnZygoteError> {
        let mut fds = Vec::<OwnedFd>::new();
        let mut buf = BUFFER_POOL.take();

        serialize_with_prefix(&task, &mut buf).map_err(Into::<SpawnZygoteErrorKind>::into)?;
        self.stream
            .send_all(buf.as_mut(), &[])
            .await
            .map_err(Into::<SpawnZygoteErrorKind>::into)?;

        Ok(())
    }
}
