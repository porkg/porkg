use std::{error::Error, fmt::Display};

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

pub trait ApiErrorStatus {
    fn status_code(&self) -> StatusCode;
}

pub trait ApiErrorData {
    type Data: Serialize;
    fn data(self) -> Self::Data;
}

pub trait ApiErrorDisplay {
    fn message(&self) -> String;
}

impl<T: Error> ApiErrorStatus for T {
    fn status_code(&self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

impl<T: Serialize + Error> ApiErrorData for T {
    type Data = Self;

    fn data(self) -> Self::Data {
        self
    }
}

impl<T: Display> ApiErrorDisplay for T {
    fn message(&self) -> String {
        format!("{}", self)
    }
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

impl<T: ApiErrorStatus + ApiErrorDisplay + ApiErrorData> IntoResponse for AppError<T> {
    fn into_response(self) -> axum::response::Response {
        let status = self.0.status_code();
        let mut r = Json(ErrorData {
            message: self.0.message(),
            data: self.0.data(),
        })
        .into_response();

        *r.status_mut() = status;
        r
    }
}
