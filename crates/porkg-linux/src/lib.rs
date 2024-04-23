mod clone;
mod fs;

use std::{
    fmt::{Debug, Display},
    path::Path,
};

use nix::errno::Errno;
pub use nix::unistd::Pid;

pub use clone::*;
pub use fs::*;
use thiserror::Error;

pub const NO_PATH: Option<&Path> = None::<&Path>;

pub struct Syscall;

pub(crate) mod private {
    use nix::errno::Errno;
    use std::fmt::Debug;

    pub trait ErrorKind: Clone + Debug {
        fn fmt(&self, err: Errno, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;
    }
}

pub type Result<T, E> = std::result::Result<T, Error<E>>;

impl private::ErrorKind for () {
    fn fmt(&self, err: Errno, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{err:?}: {err}")
    }
}

#[derive(Debug, Clone, Error)]
pub struct Error<K = ()>
where
    K: private::ErrorKind,
{
    kind: K,
    #[source]
    errno: Errno,
}

impl<K> Error<K>
where
    K: private::ErrorKind,
{
    fn new(kind: K, errno: Errno) -> Self {
        Self { kind, errno }
    }

    pub fn kind(&self) -> &K {
        &self.kind
    }
}

impl<K> Error<K>
where
    K: private::ErrorKind + Default,
{
    fn from_nix(errno: Errno) -> Self {
        Self {
            kind: Default::default(),
            errno,
        }
    }
}

impl<K> Display for Error<K>
where
    K: private::ErrorKind,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        private::ErrorKind::fmt(&self.kind, self.errno, f)
    }
}
