use std::time::Duration;

use porkg_linux::sandbox::SandboxProcess;
use porkg_private::{
    os::proc::IntoExitCode,
    sandbox::{SandboxOptions, SandboxTask},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

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

fn main() {
    // TODO: Move this into each process and send traces via the channels
    //
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .unwrap();

    let controller = SandboxProcess::<Task>::start().unwrap();

    async_std::task::block_on(async {
        let zygote = controller.connect().await.unwrap();
        zygote.spawn_async(Task, &[]).await.unwrap();

        async_std::task::sleep(Duration::from_secs(10)).await;
    })
}
