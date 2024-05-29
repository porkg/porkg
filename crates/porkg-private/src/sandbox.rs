use std::future::Future;

use crate::os::proc::IntoExitCode;

bitflags::bitflags! {
    #[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct SandboxFlags: u64 {
        /// The sandbox will not be able to access the network.
        const NETWORK_ISOLATION = 0b0001;
    }
}

#[derive(Default, Debug, Clone, PartialEq, Hash)]
pub struct SandboxOptions {
    flags: SandboxFlags,
}

impl SandboxOptions {
    pub fn flags(&self) -> SandboxFlags {
        self.flags
    }

    pub fn with_network_isolation(&mut self, isolate: bool) -> &mut Self {
        if isolate {
            self.flags.insert(SandboxFlags::NETWORK_ISOLATION)
        } else {
            self.flags.remove(SandboxFlags::NETWORK_ISOLATION)
        }
        self
    }
}

pub trait SandboxTask: crate::ser::Serialize + crate::ser::Deserialize {
    type ExecuteError: IntoExitCode + std::error::Error;
    fn execute() -> impl Send + Future<Output = Result<(), Self::ExecuteError>>;

    type CreateSandboxOptionsError: std::error::Error
        + crate::ser::Serialize
        + crate::ser::Deserialize;
    fn create_sandbox_options(
    ) -> impl Send + Future<Output = Result<SandboxOptions, Self::CreateSandboxOptionsError>>;
}
