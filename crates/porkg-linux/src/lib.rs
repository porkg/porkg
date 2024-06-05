mod clone;
mod fs;
mod proc;
pub mod sandbox;

use private::{Syscall, NO_PATH};

#[doc(hidden)]
pub mod __test {
    pub use super::clone::*;
    pub use super::fs::*;
    pub use super::private::{Syscall, NO_PATH};
    pub use super::proc::*;
}

pub(crate) mod private {
    use std::path::Path;

    pub struct Syscall;
    pub const NO_PATH: Option<&Path> = None::<&Path>;
}
