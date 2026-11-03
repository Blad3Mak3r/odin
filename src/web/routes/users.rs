//! Administrator-only account management.

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::db::instance_access;
use crate::db::users::{self, Role, User, UserError};
use crate::web::audit::{self, AuditKind};
use crate::web::auth::{
    AuthUser, ClientIp, hash_password, normalize_email, optional_username, random_token,
    validate_password,
};
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::invitations::{self, Delivery, Resent};
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
    /// Left out, the user is invited by email and chooses their own password.
    #[serde(default)]
    pub password: Option<String>,
    pub role: Role,
    /// The new user must replace this password at first sign-in. Defaults to
    /// true; meaningless for an invitation.
    #[serde(default)]
    pub require_change: Option<bool>,
    /// Instances a viewer or operator can reach from the start.
    #[serde(default)]
    pub instance_ids: Vec<String>,
}

#[derive(Serialize)]
pub struct CreatedUser {
    #[serde(flatten)]
    pub user: User,
    /// Set when the user was invited: whether the mail server took the message.
    pub invitation_delivery: Option<Delivery>,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    Json(req): Json<CreateUser>,
) -> ApiResult<Json<CreatedUser>> {
    auth.require_admin()?;
    let email = normalize_email(&req.email)?;
    let username = optional_username(req.username.as_deref())?;
    let invite = req.password.is_none();
    match &req.password {
        Some(password) => {
            validate_password(password, &[&email, username.as_deref().unwrap_or_default()])?
        }
        None => invitations::ensure_available(&state)?,
    }
    let db = state.db.clone();
    let must_change = !invite && req.require_change.unwrap_or(true);
    let user = state
        .hashing
        .run(move || {
            // An invited user has no password yet: store one nobody knows.
            let password = req.password.unwrap_or_else(random_token);
            let hash = hash_password(&password)?;
            let user = users::create(
                &db,
                &email,
                username.as_deref(),
                &hash,
                req.role,
                must_change,
            )?;
            if let Err(err) = instance_access::replace(&db, &user.email, &req.instance_ids) {
                // An account must not exist half-configured.
                users::delete(&db, &user.email)?;
                return Err(err);
            }
            Ok(user)
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
    if !invite {
        return Ok(Json(CreatedUser {
            user,
            invitation_delivery: None,
        }));
    }
    let delivery =
        match invitations::issue_and_send(&state, &auth.user.email, &user.email, ip).await {
            Ok(delivery) => delivery,
            Err(err) => {
                // No link was stored, so the account could never be used.
                let (db, email) = (state.db.clone(), user.email.clone());
                run_blocking(move || users::delete(&db, &email)).await?;
                return Err(err);
            }
        };
    let (db, email) = (state.db.clone(), user.email.clone());
    let user = run_blocking(move || users::find_by_email(&db, &email))
        .await?
        .unwrap_or(user);
    Ok(Json(CreatedUser {
        user,
        invitation_delivery: Some(delivery),
    }))
}

/// Mails a fresh link to a user who has not accepted their invitation yet.
pub async fn resend_invitation(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    Path(email): Path<String>,
) -> ApiResult<Json<Resent>> {
    auth.require_admin()?;
    invitations::ensure_available(&state)?;
    let email = normalize_email(&email)?;
    let (db, target) = (state.db.clone(), email.clone());
    let pending = run_blocking(move || crate::db::password_links::pending(&db, &target)).await?;
    if !pending {
        return Err(BadRequest("this account has no pending invitation".into()).into());
    }
    let invitation_delivery =
        invitations::issue_and_send(&state, &auth.user.email, &email, ip).await?;
    Ok(Json(Resent {
        invitation_delivery,
    }))
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
        validate_password(password, &[&email])?;
    }
    let own_account = email.eq_ignore_ascii_case(&auth.user.email);
    let must_change = req.require_change.unwrap_or(!own_account);
    let (db, target) = (state.db.clone(), email.clone());
    let (role, password, disabled) = (req.role, req.password.is_some(), req.disabled);
    let revoked = state
        .hashing
        .run(move || {
            let mut revoked = Vec::new();
            // Only the owner may reset the owner's password.
            if req.password.is_some()
                && !own_account
                && users::find_by_email(&db, &target)?.is_some_and(|user| user.is_owner)
            {
                return Err(UserError::Owner.into());
            }
            if let Some(role) = req.role {
                users::set_role(&db, &target, role)?;
                // Streams opened under the old role must reconnect under the new one.
                revoked.extend(users::session_token_hashes(&db, &target)?);
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

#[derive(Serialize, Deserialize)]
pub struct InstanceGrants {
    pub instance_ids: Vec<String>,
}

/// The instances a viewer or operator may reach. Administrators reach all
/// of them whatever is stored here.
pub async fn get_instances(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(email): Path<String>,
) -> ApiResult<Json<InstanceGrants>> {
    auth.require_admin()?;
    let db = state.db.clone();
    let instance_ids = run_blocking(move || {
        let user = users::find_by_email(&db, &email)?.ok_or(UserError::NotFound(email))?;
        instance_access::instance_ids(&db, &user.email)
    })
    .await?;
    Ok(Json(InstanceGrants { instance_ids }))
}

pub async fn set_instances(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    Path(email): Path<String>,
    Json(req): Json<InstanceGrants>,
) -> ApiResult<StatusCode> {
    auth.require_admin()?;
    let db = state.db.clone();
    let (user, revoked) = run_blocking(move || {
        let user = users::find_by_email(&db, &email)?.ok_or(UserError::NotFound(email))?;
        instance_access::replace(&db, &user.email, &req.instance_ids)?;
        let revoked = users::session_token_hashes(&db, &user.email)?;
        Ok((user, revoked))
    })
    .await?;
    // Open event streams were filtered by the old grants; closing them makes
    // the dashboard reconnect with the new ones.
    state.sessions.revoke(&revoked);
    audit::record(
        &state,
        AuditKind::InstanceAccessChanged,
        Some(&auth.user.email),
        Some(&user.email),
        ip,
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
