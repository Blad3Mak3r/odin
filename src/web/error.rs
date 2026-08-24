//! A single JSON error shape for the whole API: `{"error": "..."}"`, with the
//! HTTP status derived from the underlying domain error where one is known
//! (e.g. `InstanceError::NotFound` -> 404), falling back to 500.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::instance::InstanceError;

pub struct ApiError(anyhow::Error);

pub type ApiResult<T> = Result<T, ApiError>;

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        ApiError(err.into())
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = classify(&self.0);
        let body = ErrorBody {
            error: format!("{:#}", self.0),
        };
        (status, Json(body)).into_response()
    }
}

fn classify(err: &anyhow::Error) -> StatusCode {
    match err.downcast_ref::<InstanceError>() {
        Some(InstanceError::NotFound(_)) => StatusCode::NOT_FOUND,
        Some(InstanceError::InvalidName(_)) => StatusCode::BAD_REQUEST,
        Some(
            InstanceError::AlreadyRunning(_)
            | InstanceError::AlreadyExists(_)
            | InstanceError::NotRunning(_),
        ) => StatusCode::CONFLICT,
        None => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Runs a blocking domain call on the blocking thread pool and flattens both
/// the domain `anyhow::Result` and a task panic into a single `ApiError`, so
/// route handlers can just write `run_blocking(move || ...).await?`.
pub async fn run_blocking<F, T>(f: F) -> ApiResult<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(result) => Ok(result?),
        Err(join_err) => Err(anyhow::anyhow!("background task panicked: {join_err}").into()),
    }
}
