use std::{
    fmt,
    io::{Read as _, Write as _},
    marker::PhantomData,
    os::{
        fd::OwnedFd,
        unix::{net::UnixStream, prelude::RawFd},
    },
    sync::Arc,
};

use anyhow::Context as _;
use async_lock::Mutex;
use porkg_private::{
    io::{DomainSocket, DomainSocketAsync as _, DomainSocketAsyncExt, SocketMessageError},
    os::proc::{ChildProcess, IntoExitCode},
    sandbox::{SandboxOptions, SandboxTask},
};
use thiserror::Error;
use tokio::net::UnixStream as UnixStreamAsync;

use crate::{
    clone::{CloneError, CloneFlags, CloneSyscall},
    private::Syscall,
    proc::{IdMapping, IdMappingTools, ProcSyscall},
};

#[derive(Debug, Error)]
pub enum StartControllerProcessError {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Clone(#[from] CloneError),
}

#[derive(Debug, Error)]
pub enum ConnectControllerError {
    #[error(transparent)]
    IO(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum CreateSandboxError {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Serialization(#[from] porkg_private::ser::Error),
}

impl From<SocketMessageError> for CreateSandboxError {
    fn from(value: SocketMessageError) -> Self {
        match value {
            SocketMessageError::IO(i) => Self::IO(i),
            SocketMessageError::Serialize(i) => Self::Serialization(i),
        }
    }
}

const CMD_HELLO: u8 = 0x1;
const CMD_START: u8 = 0x2;

fn make_async(s: UnixStream) -> std::io::Result<UnixStreamAsync> {
    s.set_nonblocking(true)?;
    UnixStreamAsync::from_std(s)
}

#[derive(Debug)]
pub struct SandboxProcess<T: SandboxTask, S: CloneSyscall + ProcSyscall = Syscall> {
    stream: UnixStream,
    proc: ChildProcess,
    _p: PhantomData<(T, S)>,
}

impl<T: SandboxTask, S: CloneSyscall + ProcSyscall> SandboxProcess<T, S> {
    #[tracing::instrument]
    pub fn start() -> Result<Self, StartControllerProcessError> {
        let tools = S::find_tools();
        let (parent, child) = UnixStream::pair()
            .inspect(|_| tracing::trace!("created socket pair for controller communication"))
            .inspect_err(|error| {
                tracing::error!(
                    ?error,
                    "failed to create socket pair for controller communication"
                )
            })?;

        let cb = move || match child.try_clone() {
            Ok(child) => zygote_main::<T, S>(child, tools.clone()),
            Err(e) => Err(anyhow::anyhow!("failed to clone child socket: {0}", e)),
        };

        let zygote: ChildProcess = S::clone(cb, CloneFlags::empty())
            .inspect(|pid| tracing::trace!(?pid, "started controller process"))
            .inspect_err(|error| tracing::error!(?error, "failed to start controller process"))?
            .into();

        Ok(Self {
            stream: parent,
            proc: zygote,
            _p: PhantomData,
        })
    }

    #[tracing::instrument(skip_all)]
    pub async fn connect(self) -> Result<SandboxController<T, S>, ConnectControllerError> {
        let stream = make_async(self.stream)
            .inspect_err(|error| tracing::error!(?error, "failed to make socket async"))?;
        stream
            .send_all(&mut &[CMD_HELLO][..], &[])
            .await
            .inspect(|_| tracing::trace!("sent connect message"))
            .inspect_err(|error| tracing::trace!(?error, "failed to send connect message"))?;
        let state = Arc::new(Mutex::new(State {
            stream,
            _proc: self.proc,
            _p: PhantomData,
        }));
        Ok(SandboxController(state))
    }
}

struct State<T: SandboxTask, S: CloneSyscall + ProcSyscall = Syscall> {
    stream: UnixStreamAsync,
    _proc: ChildProcess,
    _p: PhantomData<(T, S)>,
}

pub struct SandboxController<T: SandboxTask, S: CloneSyscall + ProcSyscall = Syscall>(
    Arc<Mutex<State<T, S>>>,
);

impl<T: SandboxTask, S: CloneSyscall + ProcSyscall> Clone for SandboxController<T, S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: SandboxTask, S: CloneSyscall + ProcSyscall> std::fmt::Debug for SandboxController<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = self.0.lock_arc_blocking();
        f.debug_struct("SandboxController")
            .field("stream", &v.stream)
            .field("_proc", &v._proc)
            .field("_p", &v._p)
            .finish()
    }
}

impl<T: SandboxTask, S: CloneSyscall + ProcSyscall> SandboxController<T, S> {
    #[tracing::instrument(skip_all)]
    pub async fn spawn_async(&self, task: T, fds: &[RawFd]) -> Result<(), CreateSandboxError> {
        let state = self.0.lock_arc().await;
        state
            .stream
            .send_all(&mut &[CMD_START][..], &[])
            .await
            .inspect_err(|error| tracing::trace!(?error, "failed to send start message"))
            .map_err(CreateSandboxError::from)?;
        state
            .stream
            .send_message(&task, fds)
            .await
            .inspect(|_| tracing::trace!("sent start message"))
            .inspect_err(|error| tracing::trace!(?error, "failed to send start message"))
            .map_err(CreateSandboxError::from)?;

        Ok(())
    }
}

fn zygote_main<T: SandboxTask, S: CloneSyscall + ProcSyscall>(
    host: UnixStream,
    tools: IdMappingTools,
) -> anyhow::Result<()> {
    let mut cmd_buf = [0u8; 1];
    host.recv_exact(&mut &mut cmd_buf[..], &mut Vec::new())
        .context("while reading command from host")?;

    loop {
        let mut fds = Vec::new();

        host.recv_exact(&mut &mut cmd_buf[..], &mut fds)
            .context("while reading command from host")?;

        fds.clear();
        match cmd_buf[0] {
            CMD_START => {
                tracing::trace!("received start message");
                let task: T = host
                    .recv_message(&mut fds)
                    .context("while reading the task from the host")?;
                let opts = task.create_sandbox_options();
                start_worker::<T, S>(task, fds, opts, tools.clone())?;
            }
            other => anyhow::bail!("unknown command {other}"),
        }
    }
}

fn clone_fds(fds: &[OwnedFd]) -> Vec<OwnedFd> {
    fds.iter().map(|v| v.try_clone().unwrap()).collect()
}

fn start_worker<T: SandboxTask, S: CloneSyscall + ProcSyscall>(
    task: T,
    fds: Vec<OwnedFd>,
    opts: SandboxOptions,
    tools: IdMappingTools,
) -> anyhow::Result<()> {
    let (mut host, child) =
        UnixStream::pair().context("while creating uds for supervisor communication")?;

    let cb = move || {
        worker_main::<T, S>(
            &task,
            clone_fds(&fds[..]),
            opts.clone(),
            child.try_clone().unwrap(),
        )
    };

    let pid = S::clone(
        cb,
        CloneFlags::NEWPID | CloneFlags::NEWNS | CloneFlags::NEWUSER,
    )
    .context("while creating supervisor process")?;

    S::write_mappings(
        Some(pid),
        [IdMapping::current_user_to_root()],
        [IdMapping::current_group_to_root()],
        tools,
    )
    .context("while writing mappings")?;

    host.write_all(&[0x01u8][..])
        .context("while informing supervisor to proceed")?;

    Ok(())
}

#[derive(Debug, Error)]
enum WorkerError<T> {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Task(T),
    #[error(transparent)]
    SetId(#[from] super::proc::SetIdsError),
}

impl<T: IntoExitCode + fmt::Debug> IntoExitCode for WorkerError<T> {
    fn report(&self) -> i32 {
        match self {
            WorkerError::Task(t) => t.report(),
            other => {
                tracing::error!(error = ?other);
                -1
            }
        }
    }
}

fn worker_main<T: SandboxTask, S: ProcSyscall>(
    task: &T,
    fds: Vec<OwnedFd>,
    opts: SandboxOptions,
    mut host: UnixStream,
) -> Result<(), WorkerError<T::ExecuteError>> {
    let mut buf = [0u8; 1];

    host.read_exact(&mut buf)
        .inspect(|_| tracing::trace!("received signal to start"))
        .inspect_err(|error| tracing::error!(?error, "failed to read signal from host"))?;
    S::set_ids(opts.sandbox_uid(), opts.sandbox_gid())
        .inspect(|_| tracing::trace!("updated uid and gid"))
        .inspect_err(|error| tracing::error!(?error, "failed to update uid and gid"))?;

    task.execute(fds).map_err(WorkerError::Task)
}
