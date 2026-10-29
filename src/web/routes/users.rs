//! Administrator-only account management.

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::db::users::{self, Role, User};
use crate::web::audit::{self, AuditKind};
use crate::web::auth::{
    AuthUser, ClientIp, hash_password, normalize_email, optional_username, validate_password,
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
    /// The new user must replace this password at first sign-in. Defaults to true.
    #[serde(default)]
    pub require_change: Option<bool>,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    Json(req): Json<CreateUser>,
) -> ApiResult<Json<User>> {
    auth.require_admin()?;
    let email = normalize_email(&req.email)?;
    let username = optional_username(req.username.as_deref())?;
    validate_password(&req.password)?;
    let db = state.db.clone();
    let must_change = req.require_change.unwrap_or(true);
    let user = run_blocking(move || {
        let hash = hash_password(&req.password)?;
        users::create(
            &db,
            &email,
            username.as_deref(),
            &hash,
            req.role,
            must_change,
        )
    })
    .await?;
    let role = format!("{:?}", user.role).to_lowercase();
    audit::record(
        &state,
        AuditKind::UserCreated,
        Some(&auth.user.email),
        Some(&user.email),
        ip,
        Some(&role),
    )
    .await;
    Ok(Json(user))
}

#[derive(Deserialize)]
pub struct UpdateUser {
    pub role: Option<Role>,
    pub password: Option<String>,
    /// With `password`: the owner must replace it at next sign-in. Defaults
    /// to true, except when an administrator resets their own password.
    #[serde(default)]
    pub require_change: Option<bool>,
    /// Block (`true`) or restore (`false`) the account's ability to sign in.
    #[serde(default)]
    pub disabled: Option<bool>,
}

pub async fn update(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    Path(email): Path<String>,
    Json(req): Json<UpdateUser>,
) -> ApiResult<StatusCode> {
    auth.require_admin()?;
    if req.role.is_none() && req.password.is_none() && req.disabled.is_none() {
        return Err(BadRequest("nothing to update".into()).into());
    }
    if req.disabled == Some(true) && email.eq_ignore_ascii_case(&auth.user.email) {
        return Err(BadRequest("you cannot disable your own account".into()).into());
    }
    if let Some(password) = &req.password {
        validate_password(password)?;
    }
    let own_account = email.eq_ignore_ascii_case(&auth.user.email);
    let must_change = req.require_change.unwrap_or(!own_account);
    let (db, target) = (state.db.clone(), email.clone());
    let (role, password, disabled) = (req.role, req.password.is_some(), req.disabled);
    let revoked = run_blocking(move || {
        let mut revoked = Vec::new();
        if let Some(role) = req.role {
            users::set_role(&db, &target, role)?;
        }
        if let Some(disabled) = req.disabled {
            revoked.extend(users::set_disabled(&db, &target, disabled)?);
        }
        if let Some(password) = req.password {
            revoked.extend(users::set_password(
                &db,
                &target,
                &hash_password(&password)?,
                must_change,
            )?);
        }
        Ok(revoked)
    })
    .await?;
    state.sessions.revoke(&revoked);
    let actor = Some(auth.user.email.as_str());
    if let Some(role) = role {
        let role = format!("{role:?}").to_lowercase();
        audit::record(
            &state,
            AuditKind::RoleChanged,
            actor,
            Some(&email),
            ip,
            Some(&role),
        )
        .await;
    }
    if let Some(disabled) = disabled {
        let kind = if disabled {
            AuditKind::AccountDisabled
        } else {
            AuditKind::AccountEnabled
        };
        audit::record(&state, kind, actor, Some(&email), ip, None).await;
    }
    if password {
        audit::record(
            &state,
            AuditKind::PasswordReset,
            actor,
            Some(&email),
            ip,
            None,
        )
        .await;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    Path(email): Path<String>,
) -> ApiResult<StatusCode> {
    auth.require_admin()?;
    let (db, target) = (state.db.clone(), email.clone());
    let revoked = run_blocking(move || users::delete(&db, &target)).await?;
    state.sessions.revoke(&revoked);
    audit::record(
        &state,
        AuditKind::UserDeleted,
        Some(&auth.user.email),
        Some(&email),
        ip,
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
