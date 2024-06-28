pub mod fork;
pub use porkg_test_macros::fork_test;
use tracing::{subscriber, Level};

pub fn init_test_logging() {
    subscriber::set_global_default(
        tracing_subscriber::fmt()
            .pretty()
            .with_test_writer()
            .with_max_level(Level::TRACE)
            .finish(),
    )
    .unwrap();
}
