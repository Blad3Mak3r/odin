//! Administrator-only account management.

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::db::users::{self, Role, User};
use crate::web::auth::{
    AuthUser, hash_password, normalize_email, optional_username, validate_password,
};
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
    pub email: String,
    #[serde(default)]
    pub username: Option<String>,
    pub password: String,
    pub role: Role,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<CreateUser>,
) -> ApiResult<Json<User>> {
    auth.require_admin()?;
    let email = normalize_email(&req.email)?;
    let username = optional_username(req.username.as_deref())?;
    validate_password(&req.password)?;
    let db = state.db.clone();
    let user = run_blocking(move || {
        let hash = hash_password(&req.password)?;
        users::create(&db, &email, username.as_deref(), &hash, req.role)
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
    Path(email): Path<String>,
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
    let revoked = run_blocking(move || {
        let mut revoked = Vec::new();
        if let Some(role) = req.role {
            users::set_role(&db, &email, role)?;
        }
        if let Some(password) = req.password {
            revoked = users::set_password(&db, &email, &hash_password(&password)?)?;
        }
        Ok(revoked)
    })
    .await?;
    state.sessions.revoke(&revoked);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(email): Path<String>,
) -> ApiResult<StatusCode> {
    auth.require_admin()?;
    let db = state.db.clone();
    let revoked = run_blocking(move || users::delete(&db, &email)).await?;
    state.sessions.revoke(&revoked);
    Ok(StatusCode::NO_CONTENT)
}
