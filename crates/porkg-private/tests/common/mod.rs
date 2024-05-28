use tracing::{subscriber, Level};

pub fn setup() {
    subscriber::set_global_default(
        tracing_subscriber::fmt()
            .pretty()
            .with_test_writer()
            .with_max_level(Level::TRACE)
            .finish(),
    )
    .unwrap();
}
