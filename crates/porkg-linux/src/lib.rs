mod clone;
mod fs;
mod net;
mod proc;
mod sandbox;

use nix::errno::Errno;
use private::{Syscall, NO_PATH};
use std::fmt::{Debug, Display};
use thiserror::Error;

#[doc(hidden)]
pub mod private {
    use nix::errno::Errno;
    use std::fmt::Debug;
    use std::path::Path;

    pub use super::clone::*;
    pub use super::fs::*;

    pub struct Syscall;
    pub const NO_PATH: Option<&Path> = None::<&Path>;

    pub trait ErrorKind: Clone + Debug {
        fn fmt(&self, err: Errno, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;
    }

    impl ErrorKind for () {
        fn fmt(&self, err: Errno, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{err:?}: {err}")
        }
    }
}

pub type Result<T, E> = std::result::Result<T, Error<E>>;

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

    fn change_kind<T: private::ErrorKind>(&self, kind: T) -> Error<T> {
        Error {
            kind,
            errno: self.errno,
        }
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

    fn from_any<E>(_: E) -> Self {
        Self {
            kind: Default::default(),
            errno: Errno::UnknownErrno,
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
