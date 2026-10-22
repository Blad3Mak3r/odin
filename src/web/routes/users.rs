//! Administrator-only account management.

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::db::users::{self, Role, User};
use crate::web::auth::{AuthUser, hash_password, validate_password, validate_username};
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

pub async fn list(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> ApiResult<Json<Vec<User>>> {
    auth.require_admin()?;
    let db = state.db.clone();
    Ok(Json(run_blocking(move || users::list(&db)).await?))
}

#[derive(Deserialize)]
pub struct CreateUser {
    pub username: String,
    pub password: String,
    pub role: Role,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<CreateUser>,
) -> ApiResult<Json<User>> {
    auth.require_admin()?;
    validate_username(&req.username)?;
    validate_password(&req.password)?;
    let db = state.db.clone();
    let user = run_blocking(move || {
        users::create(&db, &req.username, &hash_password(&req.password)?, req.role)
    })
    .await?;
    Ok(Json(user))
}

#[derive(Deserialize)]
pub struct UpdateUser {
    pub role: Option<Role>,
    pub password: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(req): Json<UpdateUser>,
) -> ApiResult<StatusCode> {
    auth.require_admin()?;
    if req.role.is_none() && req.password.is_none() {
        return Err(BadRequest("nothing to update".into()).into());
    }
    if let Some(password) = &req.password {
        validate_password(password)?;
    }
    let db = state.db.clone();
    run_blocking(move || {
        if let Some(role) = req.role {
            users::set_role(&db, &id, role)?;
        }
        if let Some(password) = req.password {
            users::set_password(&db, &id, &hash_password(&password)?)?;
        }
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    auth.require_admin()?;
    let db = state.db.clone();
    run_blocking(move || users::delete(&db, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
