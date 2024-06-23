mod clone;
mod fs;
mod proc;
pub mod sandbox;

use private::{Syscall, NO_PATH};

pub(crate) mod private {
    use std::path::Path;

    pub struct Syscall;
    pub const NO_PATH: Option<&Path> = None::<&Path>;
}
