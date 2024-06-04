use std::{future::Future, os::fd::OwnedFd};

use nix::unistd::{Gid, Uid};

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
    sandbox_uid: u32,
    sandbox_gid: u32,
}

impl SandboxOptions {
    pub fn flags(&self) -> SandboxFlags {
        self.flags
    }

    pub fn sandbox_uid(&self) -> Uid {
        Uid::from_raw(self.sandbox_uid)
    }

    pub fn sandbox_gid(&self) -> Gid {
        Gid::from_raw(self.sandbox_gid)
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

pub trait SandboxTask:
    crate::ser::Serialize + crate::ser::Deserialize + Send + Sync + 'static
{
    type ExecuteError: IntoExitCode + std::error::Error;
    fn execute(&self, fds: impl AsRef<[OwnedFd]>) -> Result<(), Self::ExecuteError>;
    fn create_sandbox_options(&self) -> SandboxOptions;
}
