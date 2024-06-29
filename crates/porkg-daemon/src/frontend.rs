use tokio_util::sync::CancellationToken;

use crate::SetupState;

mod api;
mod serve;

pub async fn host(state: SetupState, cancellation_token: CancellationToken) -> anyhow::Result<()> {
    let app = axum::Router::new().nest("/api/v1", api::v1::build(&state));

    serve::serve(&state.config.bind, app, cancellation_token).await
}
