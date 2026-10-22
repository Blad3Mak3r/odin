//! A single JSON error shape for the whole API: `{"error": "..."}"`, with the
//! HTTP status derived from the underlying domain error where one is known
//! (e.g. `InstanceError::NotFound` -> 404), falling back to 500.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;

use crate::backup::BackupError;
use crate::db::users::UserError;
use crate::db::webhooks::WebhookError;
use crate::game::rust::access_lists::RustAccessListError;
use crate::instance::InstanceError;
use crate::instance::lists::ListsError;
use crate::mods::config::ConfigFileError;
use crate::mods::curseforge::CurseForgeError;
use crate::mods::nexus::NexusError;
use crate::web::auth::{Forbidden, InvalidBootstrapToken, InvalidCredentials, TooManyAttempts};
use crate::web::routes::saves::SaveFileError;

/// A catch-all for ad-hoc input validation in route handlers that doesn't
/// warrant its own domain error type (e.g. "password too short"). Always
/// maps to 400.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct BadRequest(pub String);

pub struct ApiError(pub(crate) anyhow::Error);

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
    match err.downcast_ref::<SaveFileError>() {
        Some(SaveFileError::InvalidPath) => return StatusCode::BAD_REQUEST,
        Some(SaveFileError::NotFound(_)) => return StatusCode::NOT_FOUND,
        None => {}
    }
    if err.downcast_ref::<ListsError>().is_some()
        || err.downcast_ref::<RustAccessListError>().is_some()
        || err.downcast_ref::<BadRequest>().is_some()
        || err
            .downcast_ref::<crate::podman::shared_install::MigrationRefused>()
            .is_some()
        || err
            .downcast_ref::<crate::db::game_instances::InvalidRustConfig>()
            .is_some()
        || err
            .downcast_ref::<crate::db::game_instances::InvalidGenericConfig>()
            .is_some()
    {
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
    match err.downcast_ref::<UserError>() {
        Some(UserError::NotFound(_)) => return StatusCode::NOT_FOUND,
        Some(_) => return StatusCode::CONFLICT,
        None => {}
    }
    if err.downcast_ref::<InvalidCredentials>().is_some() {
        return StatusCode::UNAUTHORIZED;
    }
    if err.downcast_ref::<Forbidden>().is_some()
        || err.downcast_ref::<InvalidBootstrapToken>().is_some()
    {
        return StatusCode::FORBIDDEN;
    }
    if err.downcast_ref::<TooManyAttempts>().is_some() {
        return StatusCode::TOO_MANY_REQUESTS;
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
    match err.downcast_ref::<CurseForgeError>() {
        Some(
            CurseForgeError::ApiKeyMissing
            | CurseForgeError::InvalidReference(_)
            | CurseForgeError::WrongGame(_),
        ) => return StatusCode::BAD_REQUEST,
        Some(CurseForgeError::NotFound(_)) => return StatusCode::NOT_FOUND,
        // Not 401: the dashboard must not read a bad operator key as the
        // user's own session being unauthorized.
        Some(CurseForgeError::Rejected(_)) => return StatusCode::BAD_GATEWAY,
        None => {}
    }
    match err.downcast_ref::<crate::odin_update::SystemUpdateError>() {
        Some(crate::odin_update::SystemUpdateError::Unsupported) => {
            return StatusCode::UNPROCESSABLE_ENTITY;
        }
        Some(crate::odin_update::SystemUpdateError::AlreadyRunning) => {
            return StatusCode::CONFLICT;
        }
        None => {}
    }
    match err.downcast_ref::<InstanceError>() {
        Some(InstanceError::NotFound(_)) => StatusCode::NOT_FOUND,
        Some(InstanceError::InvalidName(_) | InstanceError::InvalidWorldName) => {
            StatusCode::BAD_REQUEST
        }
        Some(
            InstanceError::AlreadyRunning(_)
            | InstanceError::AlreadyExists(_)
            | InstanceError::NotRunning(_)
            | InstanceError::ModsLocked(_)
            | InstanceError::TransitionInProgress(_),
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
