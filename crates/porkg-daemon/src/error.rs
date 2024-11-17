use std::fmt::Display;

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

pub trait ApiError: Display {
    type Data: Serialize;

    fn status_code(&self) -> StatusCode;
    fn data(self) -> Self::Data;
}

pub struct AppError<T>(T);

impl<E> From<E> for AppError<E> {
    fn from(value: E) -> Self {
        AppError(value)
    }
}

#[derive(Serialize)]
struct ErrorData<T: Serialize> {
    message: String,
    data: T,
}

impl ApiError for anyhow::Error {
    type Data = ();
    fn status_code(&self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }
    fn data(self) -> Self::Data {}
}

impl<T: ApiError + std::fmt::Display> IntoResponse for AppError<T> {
    fn into_response(self) -> axum::response::Response {
        let status = self.0.status_code();
        let mut r = Json(ErrorData {
            message: format!("{}", self.0),
            data: self.0.data(),
        })
        .into_response();

        *r.status_mut() = status;
        r
    }
}
