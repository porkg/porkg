use tokio_util::sync::CancellationToken;

use crate::SetupState;

use self::serve::{HostOptions, UnixHostOptions};

mod api;
mod serve;

pub async fn host(state: SetupState, cancellation_token: CancellationToken) -> anyhow::Result<()> {
    let app = axum::Router::new().nest("/api/v1", api::v1::build(&state));

    serve::serve(
        HostOptions {
            unix: UnixHostOptions {
                path: "/var/run/user/1000/porkg.socket".into(),
            },
            tcp: None,
        },
        app,
        cancellation_token,
    )
    .await
}
