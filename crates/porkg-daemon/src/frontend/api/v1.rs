use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};
use porkg_linux::sandbox::SandboxController;

use crate::{backend::BuildTask, config::Config};

mod build;

#[derive(Debug, Clone)]
struct SharedState {
    controller: SandboxController<BuildTask>,
    config: Arc<Config>,
}

async fn root() -> String {
    "Hello World".to_string()
}

pub fn build(state: &crate::SetupState) -> Router<()> {
    Router::new()
        .route("/", get(root))
        .route("/build", post(build::post))
        .with_state(SharedState {
            controller: state.controller.clone(),
            config: state.config.clone(),
        })
}
