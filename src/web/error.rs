//! A single JSON error shape for the whole API: `{"error": "..."}"`, with the
//! HTTP status derived from the underlying domain error where one is known
//! (e.g. `InstanceError::NotFound` -> 404), falling back to 500.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;

use crate::backup::BackupError;
use crate::db::webhooks::WebhookError;
use crate::instance::InstanceError;
use crate::instance::lists::ListsError;
use crate::mods::config::ConfigFileError;
use crate::mods::nexus::NexusError;

/// A catch-all for ad-hoc input validation in route handlers that doesn't
/// warrant its own domain error type (e.g. "password too short"). Always
/// maps to 400.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct BadRequest(pub String);

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
    if err.downcast_ref::<ListsError>().is_some() || err.downcast_ref::<BadRequest>().is_some() {
        return StatusCode::BAD_REQUEST;
    }
    match err.downcast_ref::<ConfigFileError>() {
        Some(ConfigFileError::InvalidFilename(_)) => return StatusCode::BAD_REQUEST,
        Some(ConfigFileError::NotFound(_)) => return StatusCode::NOT_FOUND,
        None => {}
    }
    if let Some(BackupError::NotFound(_)) = err.downcast_ref::<BackupError>() {
        return StatusCode::NOT_FOUND;
    }
    if let Some(WebhookError::NotFound(_)) = err.downcast_ref::<WebhookError>() {
        return StatusCode::NOT_FOUND;
    }
    match err.downcast_ref::<NexusError>() {
        Some(NexusError::ApiKeyMissing | NexusError::InvalidReference(_)) => {
            return StatusCode::BAD_REQUEST;
        }
        Some(NexusError::Unauthorized) => return StatusCode::UNAUTHORIZED,
        Some(NexusError::ModNotFound(_)) => return StatusCode::NOT_FOUND,
        Some(NexusError::DownloadUnavailable(_)) => return StatusCode::UNPROCESSABLE_ENTITY,
        None => {}
    }
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
