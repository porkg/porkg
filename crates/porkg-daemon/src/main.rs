use std::{future::Future, time::Duration};

use porkg_linux::sandbox::{SandboxController, SandboxProcess};
use porkg_private::{
    os::proc::IntoExitCode,
    sandbox::{SandboxOptions, SandboxTask},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

mod error;
mod frontend;

#[derive(Clone)]
struct SetupState {
    controller: SandboxController<Task>,
    exit: flume::Sender<Option<anyhow::Error>>,
}

#[derive(Serialize, Deserialize)]
struct Task;

#[derive(Debug, Error)]
#[error("tmp")]
struct Erro;

impl IntoExitCode for Erro {
    fn report(&self) -> i32 {
        -1
    }
}

impl SandboxTask for Task {
    type ExecuteError = Erro;

    fn create_sandbox_options(&self) -> porkg_private::sandbox::SandboxOptions {
        SandboxOptions::default()
    }

    fn execute(
        &self,
        _fds: impl AsRef<[std::os::unix::prelude::OwnedFd]>,
    ) -> Result<(), Self::ExecuteError> {
        tracing::trace!("running");
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    // TODO: Move this into each process and send traces via the channels
    //
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()?;

    let controller = SandboxProcess::<Task>::start()?;

    // cloneing when there are multiple threads is UB, so the above must occur first.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()?;

    let controller = runtime.block_on(controller.connect())?;

    let (sender, receiver) = flume::bounded(1);
    let state = SetupState {
        controller,
        exit: sender.clone(),
    };

    let cancellation_token = CancellationToken::new();
    let result = {
        let _cancel = cancellation_token.clone().drop_guard();
        exit_on_error(
            &runtime,
            frontend::host(state.clone(), cancellation_token.clone()),
            sender.clone(),
        );

        runtime.block_on(async move {
            let result = tokio::select! {
                err = receiver.recv_async() => err,
                _ = tokio::signal::ctrl_c() => return Ok(())
            };

            match result {
                Ok(Some(err)) => Err(err),
                _ => Ok(()),
            }
        })
    };

    runtime.shutdown_timeout(Duration::from_secs(5));
    result
}

fn exit_on_error(
    runtime: &Runtime,
    f: (impl 'static + Send + Future<Output = anyhow::Result<()>>),
    sender: flume::Sender<Option<anyhow::Error>>,
) {
    runtime.spawn(async move {
        let mut kill = DropKill(Some(sender.clone()));

        if let Err(error) = f.await {
            sender.try_send(Some(error)).ok();
        }

        kill.0 = None;
    });
}

struct DropKill(Option<flume::Sender<Option<anyhow::Error>>>);

impl Drop for DropKill {
    fn drop(&mut self) {
        if let Some(v) = self.0.take() {
            v.try_send(Some(anyhow::anyhow!("A panic occurred"))).ok();
        }
    }
}
