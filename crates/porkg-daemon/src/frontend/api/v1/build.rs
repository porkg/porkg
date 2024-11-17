use axum::{extract::State, Json};
use hyper::StatusCode;
use itertools::Itertools;
use porkg_model::package::LockDefinition;
use thiserror::Error;

use crate::{
    backend::BuildTask,
    error::{ApiError, AppError},
};

use super::SharedState;

#[derive(Debug, serde::Deserialize)]
pub struct BuildRequest {
    name: String,
    hash: String,
    lock: LockDefinition,
}

#[derive(Debug, Error, serde::Serialize)]
pub enum StartError {
    #[error("invalid hash provided: {hash}")]
    InvalidHash { hash: String },
    #[error("invalid dependency hash provided for {name}: {hash}")]
    InvalidDependencyHash { name: String, hash: String },
    #[error("failed to validate the build")]
    ValidationError { error: String },
}

impl ApiError for StartError {
    type Data = Self;

    fn status_code(&self) -> StatusCode {
        StatusCode::BAD_REQUEST
    }

    fn data(self) -> Self::Data {
        self
    }
}

// #[cfg_attr(test, axum_macros::debug_handler)]
pub async fn post(
    State(state): State<SharedState>,
    Json(req): Json<BuildRequest>,
) -> Result<String, AppError<StartError>> {
    let BuildRequest {
        name,
        hash,
        lock: LockDefinition {
            dependencies,
            build_dependencies,
        },
    } = req;

    let dependencies = dependencies
        .into_iter()
        .map(|(name, hash)| {
            hash.parse()
                .map(|v| (name.clone(), v))
                .map_err(|_| StartError::InvalidDependencyHash { name, hash })
        })
        .try_collect()?;

    let build_dependencies = build_dependencies
        .into_iter()
        .map(|(name, hash)| {
            hash.parse()
                .map(|v| (name.clone(), v))
                .map_err(|_| StartError::InvalidDependencyHash { name, hash })
        })
        .try_collect()?;

    let task = BuildTask {
        name,
        hash: hash.parse().map_err(|_| StartError::InvalidHash { hash })?,
        dependencies,
        build_dependencies,
    };

    task.validate(&state.config.store)
        .await
        .map_err(|error| StartError::ValidationError { error })?;

    Ok(format!("{:?}", task))
}
