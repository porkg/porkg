use std::sync::Arc;

use axum::{routing::get, Router};

struct State {}

async fn root() -> String {
    "Hello World".to_string()
}

pub fn build(state: &crate::SetupState) -> Router<()> {
    Router::new()
        .route("/", get(root))
        .with_state(Arc::new(State {}))
}
