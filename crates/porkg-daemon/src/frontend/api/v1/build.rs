use axum::{extract::State, Json};

use super::SharedState;

#[derive(Debug, serde::Deserialize)]
pub struct BuildRequest {
    name: String,
    #[serde(with = "porkg_private::ser::fromstr")]
    hash: porkg_model::hashing::SupportedHash,
}

#[cfg_attr(test, axum_macros::debug_handler)]
pub async fn post(State(state): State<SharedState>, Json(req): Json<BuildRequest>) -> String {
    "Hello World".to_string()
}
